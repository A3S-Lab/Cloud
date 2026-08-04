use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
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

    pub fn is_prerelease(&self) -> Result<bool, String> {
        Ok(!self.parse_semantic_version()?.pre.is_empty())
    }

    pub fn cmp_for_selection(&self, other: &Self) -> Result<Ordering, String> {
        let this = self.parse_semantic_version()?;
        let parsed_other = other.parse_semantic_version()?;
        Ok(this
            .cmp_precedence(&parsed_other)
            .then_with(|| self.0.cmp(&other.0)))
    }

    fn parse_semantic_version(&self) -> Result<semver::Version, String> {
        semver::Version::parse(&self.0)
            .map_err(|_| "Asset release version must be a bounded semantic version".into())
    }
}

impl fmt::Display for AssetReleaseVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}
