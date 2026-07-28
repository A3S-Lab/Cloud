use serde::{Deserialize, Serialize};
use std::fmt;

const MAX_RELEASE_VERSION_BYTES: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AssetReleaseVersion(String);

impl AssetReleaseVersion {
    pub fn parse(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if value.is_empty()
            || value.len() > MAX_RELEASE_VERSION_BYTES
            || value.contains(['\0', '\r', '\n'])
        {
            return Err("Asset release version must be a bounded semantic version".into());
        }
        let parsed = semver::Version::parse(&value)
            .map_err(|_| "Asset release version must be a bounded semantic version")?;
        if parsed.to_string() != value {
            return Err("Asset release version must use canonical semantic version syntax".into());
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AssetReleaseVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}
