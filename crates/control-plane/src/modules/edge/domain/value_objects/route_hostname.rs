use serde::{Deserialize, Serialize};
use std::net::IpAddr;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RouteHostname(String);

impl RouteHostname {
    pub fn parse(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into().trim().to_ascii_lowercase();
        if value.is_empty()
            || value.len() > 253
            || value.ends_with('.')
            || value.parse::<IpAddr>().is_ok()
            || value.split('.').any(|label| {
                label.is_empty()
                    || label.len() > 63
                    || label.starts_with('-')
                    || label.ends_with('-')
                    || !label.bytes().all(|byte| {
                        byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'
                    })
            })
        {
            return Err("route hostname must be a canonical DNS name".into());
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}
