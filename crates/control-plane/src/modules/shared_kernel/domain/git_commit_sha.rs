use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GitCommitSha(String);

impl GitCommitSha {
    pub fn parse(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into().to_ascii_lowercase();
        if !matches!(value.len(), 40 | 64) || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("Git commit SHA must be a full 40- or 64-character hexadecimal ID".into());
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for GitCommitSha {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_full_git_object_ids_and_canonicalizes_hexadecimal_case() {
        let sha = GitCommitSha::parse("A".repeat(40)).expect("full SHA-1 object ID");
        assert_eq!(sha.as_str(), "a".repeat(40));
        assert!(GitCommitSha::parse("a".repeat(64)).is_ok());
        assert!(GitCommitSha::parse("a".repeat(39)).is_err());
        assert!(GitCommitSha::parse("g".repeat(40)).is_err());
    }
}
