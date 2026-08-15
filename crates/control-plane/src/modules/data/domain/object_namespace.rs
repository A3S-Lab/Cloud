use crate::modules::shared_kernel::domain::StorageNamespaceId;
use async_trait::async_trait;
use std::path::{Component, Path};

const MAX_OBJECT_NAMESPACE_KEY_BYTES: usize = 4096;
const MAX_OBJECT_NAMESPACE_VERSION_BYTES: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectNamespaceKey(String);

impl ObjectNamespaceKey {
    pub fn parse(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        let path = Path::new(&value);
        if value.is_empty()
            || value.len() > MAX_OBJECT_NAMESPACE_KEY_BYTES
            || value.contains(['\\', '\0', '\r', '\n'])
            || path.is_absolute()
            || path
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err("object namespace key is invalid".into());
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectNamespaceVersion {
    e_tag: Option<String>,
    version: Option<String>,
}

impl ObjectNamespaceVersion {
    pub fn new(e_tag: Option<String>, version: Option<String>) -> Result<Self, String> {
        if e_tag.as_deref().is_some_and(invalid_version_token)
            || version.as_deref().is_some_and(invalid_version_token)
            || e_tag.is_none() && version.is_none()
        {
            return Err("object namespace version must contain an opaque provider token".into());
        }
        Ok(Self { e_tag, version })
    }

    pub fn e_tag(&self) -> Option<&str> {
        self.e_tag.as_deref()
    }

    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }
}

fn invalid_version_token(value: &str) -> bool {
    value.is_empty()
        || value.len() > MAX_OBJECT_NAMESPACE_VERSION_BYTES
        || value.contains(['\0', '\r', '\n'])
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectNamespaceRead {
    Found {
        body: Vec<u8>,
        version: ObjectNamespaceVersion,
    },
    Missing,
    Corrupt,
}

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum ObjectNamespaceError {
    #[error("object namespace request is invalid: {0}")]
    Invalid(String),
    #[error("object namespace conditional precondition failed: {0}")]
    Precondition(String),
    #[error("object namespace content or capability is corrupt: {0}")]
    Corrupt(String),
    #[error("object namespace capability is unsupported: {0}")]
    Unsupported(String),
    #[error("object namespace is unavailable: {0}")]
    Unavailable(String),
}

#[async_trait]
pub trait IObjectNamespace: Send + Sync {
    async fn conditional_create(
        &self,
        object_key: &ObjectNamespaceKey,
        body: Vec<u8>,
        maximum_bytes: u64,
    ) -> Result<ObjectNamespaceVersion, ObjectNamespaceError>;

    async fn conditional_overwrite(
        &self,
        object_key: &ObjectNamespaceKey,
        expected: &ObjectNamespaceVersion,
        body: Vec<u8>,
        maximum_bytes: u64,
    ) -> Result<ObjectNamespaceVersion, ObjectNamespaceError>;

    async fn read(
        &self,
        object_key: &ObjectNamespaceKey,
        maximum_bytes: u64,
    ) -> Result<ObjectNamespaceRead, ObjectNamespaceError>;

    async fn delete(&self, object_key: &ObjectNamespaceKey) -> Result<(), ObjectNamespaceError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectNamespaceProbeEvidence {
    pub namespace_id: StorageNamespaceId,
    pub conditional_create: bool,
    pub competing_create_rejected: bool,
    pub read_after_create: bool,
    pub conditional_overwrite: bool,
    pub stale_overwrite_rejected: bool,
    pub read_after_overwrite: bool,
    pub cleanup_verified: bool,
}

impl ObjectNamespaceProbeEvidence {
    pub fn validate(&self) -> Result<(), String> {
        if self.namespace_id.as_uuid().is_nil()
            || !self.conditional_create
            || !self.competing_create_rejected
            || !self.read_after_create
            || !self.conditional_overwrite
            || !self.stale_overwrite_rejected
            || !self.read_after_overwrite
            || !self.cleanup_verified
        {
            return Err("object namespace conformance evidence is incomplete".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_and_evidence_fail_closed_without_exact_tokens_or_capabilities() {
        ObjectNamespaceKey::parse("owners/counter").expect("key");
        assert!(ObjectNamespaceKey::parse("../escape").is_err());
        assert!(ObjectNamespaceKey::parse("/absolute").is_err());
        assert!(ObjectNamespaceVersion::new(None, None).is_err());
        assert!(ObjectNamespaceVersion::new(Some(String::new()), None).is_err());
        assert!(ObjectNamespaceVersion::new(Some("bad\nvalue".into()), None).is_err());
        assert!(ObjectNamespaceVersion::new(
            Some("x".repeat(MAX_OBJECT_NAMESPACE_VERSION_BYTES + 1)),
            None
        )
        .is_err());
        assert!(ObjectNamespaceVersion::new(None, Some("v1".into())).is_ok());

        let mut evidence = ObjectNamespaceProbeEvidence {
            namespace_id: StorageNamespaceId::new(),
            conditional_create: true,
            competing_create_rejected: true,
            read_after_create: true,
            conditional_overwrite: true,
            stale_overwrite_rejected: true,
            read_after_overwrite: true,
            cleanup_verified: true,
        };
        evidence.validate().expect("complete evidence");
        evidence.stale_overwrite_rejected = false;
        assert!(evidence.validate().is_err());
    }
}
