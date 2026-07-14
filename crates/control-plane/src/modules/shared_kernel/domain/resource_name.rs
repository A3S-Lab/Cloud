use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceName {
    display: String,
    key: String,
}

impl ResourceName {
    pub fn parse(value: impl Into<String>) -> Result<Self, String> {
        let display = value.into().trim().to_owned();
        if display.is_empty()
            || display.chars().count() > 63
            || display.contains(['\0', '\r', '\n'])
        {
            return Err("resource name must contain 1 to 63 visible characters".into());
        }
        let key = display.to_lowercase();
        Ok(Self { display, key })
    }

    pub fn as_str(&self) -> &str {
        &self.display
    }

    pub fn key(&self) -> &str {
        &self.key
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_display_name_and_normalizes_uniqueness_key() {
        let name = ResourceName::parse("  Production  ").expect("valid name");
        assert_eq!(name.as_str(), "Production");
        assert_eq!(name.key(), "production");
    }
}
