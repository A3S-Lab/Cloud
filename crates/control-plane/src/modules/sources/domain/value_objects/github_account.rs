use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GithubAccountId(u64);

impl GithubAccountId {
    pub fn parse(value: u64) -> Result<Self, String> {
        if value == 0 || value > i64::MAX as u64 {
            return Err("GitHub account ID must be a positive signed 64-bit integer".into());
        }
        Ok(Self(value))
    }

    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

impl fmt::Display for GithubAccountId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GithubAccountKind {
    Organization,
    User,
}

impl GithubAccountKind {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "Organization" | "organization" => Ok(Self::Organization),
            "User" | "user" => Ok(Self::User),
            _ => Err("GitHub installation account type must be Organization or User".into()),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Organization => "organization",
            Self::User => "user",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GithubLogin(String);

impl GithubLogin {
    pub fn parse(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 100
            || value.starts_with('-')
            || value.ends_with('-')
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err("GitHub login must use bounded alphanumeric and hyphen syntax".into());
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for GithubLogin {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}
