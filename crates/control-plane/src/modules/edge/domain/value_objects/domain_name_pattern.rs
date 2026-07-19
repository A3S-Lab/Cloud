use super::RouteHostname;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DomainNamePattern(String);

impl DomainNamePattern {
    pub fn parse(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into().trim().to_ascii_lowercase();
        let suffix = value.strip_prefix("*.").unwrap_or(&value);
        let hostname = RouteHostname::parse(suffix)?;
        if value.contains('*') && !value.starts_with("*.") {
            return Err("domain pattern must be an exact DNS name or one leading wildcard".into());
        }
        if value.starts_with("*.") && hostname.as_str().split('.').count() < 2 {
            return Err("wildcard domain pattern must contain a registrable suffix".into());
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_wildcard(&self) -> bool {
        self.0.starts_with("*.")
    }

    pub fn dns_suffix(&self) -> &str {
        self.0.strip_prefix("*.").unwrap_or(&self.0)
    }

    pub fn covers(&self, hostname: &RouteHostname) -> bool {
        if !self.is_wildcard() {
            return self.as_str() == hostname.as_str();
        }
        hostname
            .as_str()
            .strip_suffix(self.dns_suffix())
            .and_then(|prefix| prefix.strip_suffix('.'))
            .is_some_and(|label| !label.is_empty() && !label.contains('.'))
    }

    pub fn conflicts_with(&self, other: &Self) -> bool {
        match (self.is_wildcard(), other.is_wildcard()) {
            (false, false) => self == other,
            (true, true) => self.dns_suffix() == other.dns_suffix(),
            (true, false) => {
                RouteHostname::parse(other.as_str()).is_ok_and(|hostname| self.covers(&hostname))
            }
            (false, true) => {
                RouteHostname::parse(self.as_str()).is_ok_and(|hostname| other.covers(&hostname))
            }
        }
    }

    pub fn challenge_dns_name(&self) -> String {
        format!("_a3s-cloud-challenge.{}", self.dns_suffix())
    }
}
