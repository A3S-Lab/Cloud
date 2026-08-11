use crate::modules::shared_kernel::domain::Sha256Digest;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PluginTrustRootObjectRef(String);

impl PluginTrustRootObjectRef {
    pub fn from_digest(digest: &Sha256Digest) -> Result<Self, String> {
        let hexadecimal = digest
            .as_str()
            .strip_prefix("sha256:")
            .ok_or_else(|| "plugin trust-root digest is invalid".to_owned())?;
        Self::parse(format!("sha256/{hexadecimal}/root.json"))
    }

    pub fn parse(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        let Some(remainder) = value.strip_prefix("sha256/") else {
            return Err("plugin trust-root object reference is invalid".into());
        };
        let Some(hexadecimal) = remainder.strip_suffix("/root.json") else {
            return Err("plugin trust-root object reference is invalid".into());
        };
        Sha256Digest::parse(format!("sha256:{hexadecimal}"))?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginTrustRoot {
    object_ref: PluginTrustRootObjectRef,
    digest: Sha256Digest,
    version: u64,
}

impl PluginTrustRoot {
    pub fn new(
        object_ref: PluginTrustRootObjectRef,
        digest: Sha256Digest,
        version: u64,
    ) -> Result<Self, String> {
        let root = Self {
            object_ref,
            digest,
            version,
        };
        root.validate()?;
        Ok(root)
    }

    pub fn from_digest(digest: Sha256Digest, version: u64) -> Result<Self, String> {
        let object_ref = PluginTrustRootObjectRef::from_digest(&digest)?;
        Self::new(object_ref, digest, version)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.version == 0 || self.version > i64::MAX as u64 {
            return Err("plugin trust-root version is outside the persistent range".into());
        }
        if Sha256Digest::parse(self.digest.as_str())? != self.digest
            || PluginTrustRootObjectRef::parse(self.object_ref.as_str())? != self.object_ref
            || self.object_ref != PluginTrustRootObjectRef::from_digest(&self.digest)?
        {
            return Err("plugin trust-root object reference does not match its digest".into());
        }
        Ok(())
    }

    pub fn object_ref(&self) -> &PluginTrustRootObjectRef {
        &self.object_ref
    }

    pub fn digest(&self) -> &Sha256Digest {
        &self.digest
    }

    pub const fn version(&self) -> u64 {
        self.version
    }
}

#[cfg(test)]
mod tests {
    use super::{PluginTrustRoot, PluginTrustRootObjectRef};
    use crate::modules::shared_kernel::domain::Sha256Digest;

    #[test]
    fn object_reference_is_derived_from_the_exact_digest() {
        let digest = Sha256Digest::parse(format!("sha256:{}", "a".repeat(64))).expect("digest");
        let root = PluginTrustRoot::from_digest(digest, 7).expect("root");
        assert_eq!(
            root.object_ref().as_str(),
            format!("sha256/{}/root.json", "a".repeat(64))
        );
        assert_eq!(root.version(), 7);
    }

    #[test]
    fn mismatched_object_reference_and_zero_version_fail_closed() {
        let digest = Sha256Digest::parse(format!("sha256:{}", "a".repeat(64))).expect("digest");
        let foreign =
            PluginTrustRootObjectRef::parse(format!("sha256/{}/root.json", "b".repeat(64)))
                .expect("object reference");
        assert!(PluginTrustRoot::new(foreign, digest.clone(), 1).is_err());
        assert!(PluginTrustRoot::from_digest(digest.clone(), 0).is_err());
        assert!(PluginTrustRoot::from_digest(digest, i64::MAX as u64 + 1).is_err());
    }
}
