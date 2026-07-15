use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RoutePortName(String);

impl RoutePortName {
    pub fn parse(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 63
            || !value.bytes().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'-' | b'_' | b'.')
            })
        {
            return Err("route port name must match a declared service port".into());
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}
