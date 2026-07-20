use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GithubInstallationId(u64);

impl GithubInstallationId {
    pub fn parse(value: u64) -> Result<Self, String> {
        if value == 0 || value > i64::MAX as u64 {
            return Err("GitHub installation ID must be a positive signed 64-bit integer".into());
        }
        Ok(Self(value))
    }

    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

impl fmt::Display for GithubInstallationId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}
