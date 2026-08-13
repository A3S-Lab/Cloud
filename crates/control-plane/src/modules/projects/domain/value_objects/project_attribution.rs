use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const BUSINESS_OWNER_REFERENCE_MAX_CHARS: usize = 255;
pub const COST_ATTRIBUTION_CODE_MAX_CHARS: usize = 128;
pub const PROJECT_ATTRIBUTION_LABEL_MAX_COUNT: usize = 32;
pub const PROJECT_ATTRIBUTION_LABEL_KEY_MAX_CHARS: usize = 63;
pub const PROJECT_ATTRIBUTION_LABEL_VALUE_MAX_CHARS: usize = 255;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BusinessOwnerReference(String);

impl BusinessOwnerReference {
    pub fn parse(value: impl Into<String>) -> Result<Self, String> {
        canonical_visible(
            value.into(),
            "business owner reference",
            BUSINESS_OWNER_REFERENCE_MAX_CHARS,
        )
        .map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CostAttributionCode(String);

impl CostAttributionCode {
    pub fn parse(value: impl Into<String>) -> Result<Self, String> {
        canonical_visible(
            value.into(),
            "cost attribution code",
            COST_ATTRIBUTION_CODE_MAX_CHARS,
        )
        .map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(transparent)]
pub struct ProjectAttributionLabels(BTreeMap<String, String>);

impl ProjectAttributionLabels {
    pub fn parse(values: BTreeMap<String, String>) -> Result<Self, String> {
        if values.len() > PROJECT_ATTRIBUTION_LABEL_MAX_COUNT {
            return Err(format!(
                "project attribution labels cannot contain more than {PROJECT_ATTRIBUTION_LABEL_MAX_COUNT} entries"
            ));
        }
        let mut canonical = BTreeMap::new();
        for (key, value) in values {
            validate_label_key(&key)?;
            let value = canonical_visible(
                value,
                "project attribution label value",
                PROJECT_ATTRIBUTION_LABEL_VALUE_MAX_CHARS,
            )?;
            canonical.insert(key, value);
        }
        Ok(Self(canonical))
    }

    pub fn as_map(&self) -> &BTreeMap<String, String> {
        &self.0
    }

    pub fn into_map(self) -> BTreeMap<String, String> {
        self.0
    }
}

fn canonical_visible(value: String, field: &str, max_chars: usize) -> Result<String, String> {
    let value = value.trim().to_owned();
    let count = value.chars().count();
    if count == 0 || count > max_chars || value.chars().any(char::is_control) {
        return Err(format!(
            "{field} must contain 1 to {max_chars} visible characters"
        ));
    }
    Ok(value)
}

fn validate_label_key(value: &str) -> Result<(), String> {
    let bytes = value.as_bytes();
    let valid = !bytes.is_empty()
        && bytes.len() <= PROJECT_ATTRIBUTION_LABEL_KEY_MAX_CHARS
        && bytes[0].is_ascii_lowercase()
        && bytes.iter().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        });
    if valid {
        Ok(())
    } else {
        Err(format!(
            "project attribution label keys must start with a lowercase letter and contain at most {PROJECT_ATTRIBUTION_LABEL_KEY_MAX_CHARS} lowercase ASCII letters, digits, dots, underscores, or hyphens"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attribution_values_are_canonical_and_ordered() {
        let owner = BusinessOwnerReference::parse("  finance/platform  ").expect("owner");
        let code = CostAttributionCode::parse("  CC-1042  ").expect("code");
        let labels = ProjectAttributionLabels::parse(BTreeMap::from([
            ("service.tier".into(), "  critical  ".into()),
            ("region".into(), "global".into()),
        ]))
        .expect("labels");

        assert_eq!(owner.as_str(), "finance/platform");
        assert_eq!(code.as_str(), "CC-1042");
        assert_eq!(
            labels
                .as_map()
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["region", "service.tier"]
        );
        assert_eq!(labels.as_map()["service.tier"], "critical");
    }

    #[test]
    fn attribution_values_are_bounded_and_reject_controls() {
        assert!(BusinessOwnerReference::parse("\n").is_err());
        assert!(
            BusinessOwnerReference::parse("x".repeat(BUSINESS_OWNER_REFERENCE_MAX_CHARS + 1))
                .is_err()
        );
        assert!(CostAttributionCode::parse("cost\0code").is_err());
        assert!(ProjectAttributionLabels::parse(BTreeMap::from([(
            "Invalid".into(),
            "value".into()
        )]))
        .is_err());
        assert!(
            ProjectAttributionLabels::parse(BTreeMap::from([("valid".into(), "\r".into())]))
                .is_err()
        );
        let too_many = (0..=PROJECT_ATTRIBUTION_LABEL_MAX_COUNT)
            .map(|index| (format!("label{index}"), "value".into()))
            .collect();
        assert!(ProjectAttributionLabels::parse(too_many).is_err());
    }
}
