use a3s_runtime::contract::ArtifactRef;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{
    artifact_uri, validate_cloud_artifact, validate_lower_sha256, PLUGIN_POLICY_ACL_MEDIA_TYPE,
    PLUGIN_TRUST_ROOT_MEDIA_TYPE,
};

/// Fleet-owned request for reading the canonical A3S Use Plugin Host contract.
///
/// The response is the upstream [`a3s_use_core::PluginHostCapabilities`] type;
/// Cloud does not define or persist a parallel capability schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodePluginHostCapabilitiesRequest {
    pub schema: String,
    pub generation: u64,
}

impl NodePluginHostCapabilitiesRequest {
    pub const SCHEMA: &'static str = "a3s.cloud.plugin-host-capabilities-request.v1";

    pub fn new(generation: u64) -> Result<Self, String> {
        let request = Self {
            schema: Self::SCHEMA.into(),
            generation,
        };
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported Plugin Host capabilities request schema {:?}",
                self.schema
            ));
        }
        if self.generation == 0 {
            return Err("Plugin Host capabilities request generation must be positive".into());
        }
        Ok(())
    }
}

/// Command-bound authorization of one enrolled trust root and one policy ACL.
///
/// Cloud admits both digests into the shared node Artifact store, then binds
/// them to this Fleet command so the Agent can download and verify them
/// without a second registry-sync or policy-evaluation path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodePluginHostAuthorizeTrustRequest {
    pub schema: String,
    pub generation: u64,
    pub trust_root: ArtifactRef,
    pub policy_acl: ArtifactRef,
}

impl NodePluginHostAuthorizeTrustRequest {
    pub const SCHEMA: &'static str = "a3s.cloud.plugin-host-authorize-trust-request.v1";
    pub const TRUST_ROOT_MOUNT: &'static str = "plugin-trust-root";
    pub const POLICY_ACL_MOUNT: &'static str = "plugin-policy-acl";

    pub fn new(
        generation: u64,
        trust_root: ArtifactRef,
        policy_acl: ArtifactRef,
    ) -> Result<Self, String> {
        let request = Self {
            schema: Self::SCHEMA.into(),
            generation,
            trust_root,
            policy_acl,
        };
        request.validate()?;
        Ok(request)
    }

    pub fn from_digests(
        generation: u64,
        trust_root_digest: impl Into<String>,
        policy_digest: impl Into<String>,
    ) -> Result<Self, String> {
        let trust_root_digest = trust_root_digest.into();
        let policy_digest = policy_digest.into();
        Self::new(
            generation,
            ArtifactRef {
                uri: artifact_uri(&trust_root_digest)?,
                digest: trust_root_digest,
                media_type: PLUGIN_TRUST_ROOT_MEDIA_TYPE.into(),
            },
            ArtifactRef {
                uri: artifact_uri(&policy_digest)?,
                digest: policy_digest,
                media_type: PLUGIN_POLICY_ACL_MEDIA_TYPE.into(),
            },
        )
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported Plugin Host authorize-trust request schema {:?}",
                self.schema
            ));
        }
        if self.generation == 0 {
            return Err("Plugin Host authorize-trust request generation must be positive".into());
        }
        validate_cloud_artifact(&self.trust_root)?;
        validate_cloud_artifact(&self.policy_acl)?;
        if self.trust_root.media_type != PLUGIN_TRUST_ROOT_MEDIA_TYPE {
            return Err("Plugin Host trust-root Artifact media type is unsupported".into());
        }
        if self.policy_acl.media_type != PLUGIN_POLICY_ACL_MEDIA_TYPE {
            return Err("Plugin Host policy ACL Artifact media type is unsupported".into());
        }
        if self.trust_root.digest == self.policy_acl.digest {
            return Err("Plugin Host trust-root and policy ACL digests must differ".into());
        }
        Ok(())
    }

    pub fn artifact_for_mount(&self, mount_name: &str) -> Option<&ArtifactRef> {
        match mount_name {
            Self::TRUST_ROOT_MOUNT => Some(&self.trust_root),
            Self::POLICY_ACL_MOUNT => Some(&self.policy_acl),
            _ => None,
        }
    }

    /// Canonical digest of this authorize-trust request.
    ///
    /// Fleet Artifact downloads bind transfers to this digest so the Agent can
    /// pull the exact trust-root and policy ACL objects without a second
    /// registry-sync path.
    pub fn binding_digest(&self) -> Result<String, String> {
        self.validate()?;
        let encoded = serde_json::to_vec(self).map_err(|error| {
            format!("could not encode Plugin Host authorize-trust request: {error}")
        })?;
        Ok(format!("sha256:{:x}", Sha256::digest(encoded)))
    }
}

/// Exact digests the Agent verified after command-bound Artifact transfer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodePluginHostTrustAuthorized {
    pub schema: String,
    pub generation: u64,
    pub trust_root_digest: String,
    pub policy_digest: String,
    pub authorized_at: DateTime<Utc>,
}

impl NodePluginHostTrustAuthorized {
    pub const SCHEMA: &'static str = "a3s.cloud.plugin-host-trust-authorized.v1";

    pub fn new(
        generation: u64,
        trust_root_digest: impl Into<String>,
        policy_digest: impl Into<String>,
        authorized_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let authorized = Self {
            schema: Self::SCHEMA.into(),
            generation,
            trust_root_digest: trust_root_digest.into(),
            policy_digest: policy_digest.into(),
            authorized_at,
        };
        authorized.validate()?;
        Ok(authorized)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported Plugin Host trust-authorized schema {:?}",
                self.schema
            ));
        }
        if self.generation == 0 {
            return Err("Plugin Host trust-authorized generation must be positive".into());
        }
        validate_lower_sha256("trust-root digest", &self.trust_root_digest)?;
        validate_lower_sha256("policy digest", &self.policy_digest)?;
        if self.trust_root_digest == self.policy_digest {
            return Err("Plugin Host trust-root and policy digests must differ".into());
        }
        Ok(())
    }

    pub fn validate_for(&self, request: &NodePluginHostAuthorizeTrustRequest) -> Result<(), String> {
        self.validate()?;
        request.validate()?;
        if self.generation != request.generation
            || self.trust_root_digest != request.trust_root.digest
            || self.policy_digest != request.policy_acl.digest
        {
            return Err(
                "Plugin Host trust-authorized result does not match its authorize-trust request"
                    .into(),
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(fill: char) -> String {
        format!("sha256:{}", fill.to_string().repeat(64))
    }

    #[test]
    fn authorize_trust_request_binds_exact_media_types() {
        let request =
            NodePluginHostAuthorizeTrustRequest::from_digests(3, digest('a'), digest('b'))
                .expect("request");
        assert_eq!(request.generation, 3);
        assert_eq!(request.trust_root.media_type, PLUGIN_TRUST_ROOT_MEDIA_TYPE);
        assert_eq!(request.policy_acl.media_type, PLUGIN_POLICY_ACL_MEDIA_TYPE);
        assert!(request
            .artifact_for_mount(NodePluginHostAuthorizeTrustRequest::TRUST_ROOT_MOUNT)
            .is_some());
        assert!(NodePluginHostAuthorizeTrustRequest::from_digests(
            3,
            digest('a'),
            digest('a')
        )
        .is_err());
        let binding = request.binding_digest().expect("binding digest");
        assert!(binding.starts_with("sha256:"));
        assert_eq!(binding.len(), 71);
        assert_eq!(
            request.binding_digest().expect("stable binding"),
            binding,
            "authorize-trust binding digest must be deterministic"
        );
    }

    #[test]
    fn trust_authorized_result_must_match_request() {
        let request =
            NodePluginHostAuthorizeTrustRequest::from_digests(2, digest('c'), digest('d'))
                .expect("request");
        let authorized = NodePluginHostTrustAuthorized::new(
            2,
            digest('c'),
            digest('d'),
            DateTime::parse_from_rfc3339("2026-09-07T12:00:00Z")
                .expect("time")
                .with_timezone(&Utc),
        )
        .expect("authorized");
        authorized.validate_for(&request).expect("match");
        let drifted = NodePluginHostTrustAuthorized::new(
            2,
            digest('c'),
            digest('e'),
            authorized.authorized_at,
        )
        .expect("drifted");
        assert!(drifted.validate_for(&request).is_err());
    }
}
