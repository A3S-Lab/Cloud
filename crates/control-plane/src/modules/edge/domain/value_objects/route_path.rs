use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RoutePath(String);

impl RoutePath {
    pub fn parse(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 2048
            || !value.starts_with('/')
            || value.contains(['\0', '\r', '\n', '`', '?', '#'])
            || value.contains("//")
            || value
                .split('/')
                .any(|segment| matches!(segment, "." | ".."))
            || !valid_percent_encoding(&value)
        {
            return Err("route path must be a canonical absolute URL path prefix".into());
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn valid_percent_encoding(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len()
                || !bytes[index + 1].is_ascii_hexdigit()
                || !bytes[index + 2].is_ascii_hexdigit()
            {
                return false;
            }
            index += 3;
        } else {
            index += 1;
        }
    }
    true
}
