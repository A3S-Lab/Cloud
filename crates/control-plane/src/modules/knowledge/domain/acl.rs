use crate::modules::shared_kernel::domain::Sha256Digest;
use a3s_acl::{Block, Value};
use chrono::{DateTime, Utc};
use uuid::Uuid;

pub(super) fn validate_non_nil(value: Uuid, label: &str) -> Result<(), String> {
    if value.is_nil() {
        return Err(format!("{label} identity cannot be nil"));
    }
    Ok(())
}

pub(super) fn validate_name(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value != value.trim()
        || value.chars().count() > super::types::KNOWLEDGE_MAX_NAME_BYTES
        || value.chars().any(char::is_control)
    {
        return Err("Knowledge name must contain 1 to 63 safe characters".into());
    }
    Ok(())
}

pub(super) fn validate_tags(tags: &[String]) -> Result<(), String> {
    if tags.len() > super::types::KNOWLEDGE_MAX_TAGS {
        return Err("Knowledge tag list exceeds its bound".into());
    }
    let mut seen = std::collections::BTreeSet::new();
    for tag in tags {
        if tag.is_empty()
            || tag != tag.trim()
            || tag.chars().count() > super::types::KNOWLEDGE_MAX_TAG_BYTES
            || tag.chars().any(char::is_control)
        {
            return Err("Knowledge tag is invalid".into());
        }
        if !seen.insert(tag.clone()) {
            return Err("Knowledge tags must be unique".into());
        }
    }
    Ok(())
}

pub(super) fn exact_shape(
    block: &Block,
    name: &str,
    attributes: &[&str],
    children: &[&str],
) -> Result<(), String> {
    if block.name != name
        || !block.labels.is_empty()
        || block.attributes.len() != attributes.len()
        || block
            .attributes
            .keys()
            .any(|key| !attributes.contains(&key.as_str()))
        || block.blocks.len() != children.len()
        || block
            .blocks
            .iter()
            .any(|child| !children.contains(&child.name.as_str()))
    {
        return Err(format!("Knowledge {name} block shape is invalid"));
    }
    Ok(())
}

pub(super) fn exact_child<'a>(root: &'a Block, name: &str) -> Result<&'a Block, String> {
    let mut matches = root.blocks.iter().filter(|block| block.name == name);
    let value = matches
        .next()
        .ok_or_else(|| format!("Knowledge {name} block is required"))?;
    if matches.next().is_some() {
        return Err(format!("Knowledge {name} block must be unique"));
    }
    Ok(value)
}

pub(super) fn required_value<'a>(block: &'a Block, name: &str) -> Result<&'a Value, String> {
    block
        .attributes
        .get(name)
        .ok_or_else(|| format!("Knowledge field {name:?} is required"))
}

pub(super) fn required_string(block: &Block, name: &str) -> Result<String, String> {
    match required_value(block, name)? {
        Value::String(value) => Ok(value.clone()),
        _ => Err(format!("Knowledge field {name:?} must be a string")),
    }
}

pub(super) fn required_uuid(block: &Block, name: &str) -> Result<Uuid, String> {
    let value = Uuid::parse_str(&required_string(block, name)?)
        .map_err(|_| format!("Knowledge field {name:?} must be a UUID"))?;
    validate_non_nil(value, name)?;
    Ok(value)
}

pub(super) fn required_digest(block: &Block, name: &str) -> Result<Sha256Digest, String> {
    Sha256Digest::parse(required_string(block, name)?)
        .map_err(|_| format!("Knowledge field {name:?} must be a SHA-256 digest"))
}

pub(super) fn required_timestamp(block: &Block, name: &str) -> Result<DateTime<Utc>, String> {
    DateTime::parse_from_rfc3339(&required_string(block, name)?)
        .map_err(|_| format!("Knowledge field {name:?} must be an RFC 3339 timestamp"))
        .map(|value| value.with_timezone(&Utc))
}

pub(super) fn required_u64(block: &Block, name: &str, max: u64) -> Result<u64, String> {
    let Value::Number(value) = required_value(block, name)? else {
        return Err(format!("Knowledge field {name:?} must be an integer"));
    };
    if !value.is_finite() || value.fract() != 0.0 || *value < 0.0 || *value > max as f64 {
        return Err(format!(
            "Knowledge field {name:?} must be a bounded non-negative integer"
        ));
    }
    Ok(*value as u64)
}

pub(super) fn required_u32(block: &Block, name: &str, min: u32, max: u32) -> Result<u32, String> {
    let value = required_u64(block, name, u64::from(max))?;
    if value < u64::from(min) {
        return Err(format!(
            "Knowledge field {name:?} must be between {min} and {max}"
        ));
    }
    Ok(value as u32)
}

pub(super) fn required_string_list(block: &Block, name: &str) -> Result<Vec<String>, String> {
    match required_value(block, name)? {
        Value::List(values) => values
            .iter()
            .map(|value| match value {
                Value::String(text) => Ok(text.clone()),
                _ => Err(format!("Knowledge field {name:?} must be a string list")),
            })
            .collect(),
        _ => Err(format!("Knowledge field {name:?} must be a string list")),
    }
}

pub(super) fn required_digest_list(block: &Block, name: &str) -> Result<Vec<Sha256Digest>, String> {
    required_string_list(block, name)?
        .into_iter()
        .map(|value| {
            Sha256Digest::parse(value)
                .map_err(|_| format!("Knowledge field {name:?} must contain SHA-256 digests"))
        })
        .collect()
}
