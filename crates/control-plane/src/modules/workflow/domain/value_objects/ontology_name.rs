use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OntologyName {
    display: String,
    key: String,
}

impl OntologyName {
    pub fn parse(value: impl Into<String>) -> Result<Self, String> {
        let display = value.into();
        if display != display.trim()
            || display.is_empty()
            || display.chars().count() > 120
            || display.contains(['\0', '\r', '\n'])
        {
            return Err(
                "Ontology name must contain 1-120 safe characters without surrounding whitespace"
                    .into(),
            );
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
    fn preserves_display_and_normalizes_uniqueness_key() {
        let name = OntologyName::parse("Customer Graph").expect("valid name");
        assert_eq!(name.as_str(), "Customer Graph");
        assert_eq!(name.key(), "customer graph");
        assert!(OntologyName::parse(" Customer Graph ").is_err());
    }
}
