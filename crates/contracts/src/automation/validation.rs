use a3s_acl::builder::integer;
use a3s_acl::{Block, Value};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::Serialize;
use serde_json::{Map, Value as JsonValue};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

pub(super) const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

pub(super) fn validate_uuid(label: &str, value: Uuid) -> Result<(), String> {
    if value.is_nil() {
        Err(format!("{label} must not be nil"))
    } else {
        Ok(())
    }
}

pub(super) fn validate_digest(label: &str, value: &str) -> Result<(), String> {
    let valid = value.strip_prefix("sha256:").is_some_and(|hexadecimal| {
        hexadecimal.len() == 64
            && hexadecimal
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    });
    if valid {
        Ok(())
    } else {
        Err(format!(
            "{label} must use canonical lowercase SHA-256 syntax"
        ))
    }
}

pub(super) fn validate_name(label: &str, value: &str, maximum: usize) -> Result<(), String> {
    if value.is_empty()
        || value.len() > maximum
        || value != value.trim()
        || value.contains(['\0', '\r', '\n'])
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b' '))
    {
        return Err(format!("{label} is not a bounded portable name"));
    }
    Ok(())
}

pub(super) fn validate_single_line(label: &str, value: &str, maximum: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > maximum || value.contains(['\0', '\r', '\n']) {
        Err(format!(
            "{label} must be a bounded nonempty single-line value"
        ))
    } else {
        Ok(())
    }
}

pub(super) fn validate_event_key(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 255
        || value.split('.').count() < 2
        || value.split('.').any(|segment| {
            segment.is_empty()
                || !segment
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        })
    {
        Err("Automation event key must use lowercase dotted syntax".into())
    } else {
        Ok(())
    }
}

pub(super) fn validate_media_type(label: &str, value: &str) -> Result<(), String> {
    let mut segments = value.split('/');
    let Some(top_level) = segments.next() else {
        return Err(format!("{label} is invalid"));
    };
    let Some(subtype) = segments.next() else {
        return Err(format!("{label} is invalid"));
    };
    if segments.next().is_some()
        || value.len() > 127
        || !valid_media_type_segment(top_level)
        || !valid_media_type_segment(subtype)
    {
        return Err(format!(
            "{label} must be one canonical lowercase media type"
        ));
    }
    Ok(())
}

fn valid_media_type_segment(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(
                    byte,
                    b'!' | b'#' | b'$' | b'&' | b'^' | b'_' | b'.' | b'+' | b'-'
                )
        })
}

pub(super) fn validate_grant(value: &str) -> Result<(), String> {
    validate_single_line("Automation grant", value, 128)?;
    let Some((resource, action)) = value.split_once(':') else {
        return Err("Automation grant must use resource:action syntax".into());
    };
    if resource.is_empty()
        || action.is_empty()
        || !resource.bytes().all(valid_permission_byte)
        || !action.bytes().all(valid_permission_byte)
    {
        return Err("Automation grant must use portable resource:action syntax".into());
    }
    Ok(())
}

fn valid_permission_byte(byte: u8) -> bool {
    byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-' | b'_')
}

pub(super) fn validate_timezone(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 255
        || value.contains(['\0', '\r', '\n', '\t', ' '])
        || value.starts_with('/')
        || value.ends_with('/')
        || value.contains("//")
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'+' | b'-' | b'/'))
    {
        return Err("Automation timezone must use a bounded IANA-style name".into());
    }
    Ok(())
}

pub(super) fn validate_cron_expression(value: &str) -> Result<(), String> {
    let fields = value.split(' ').collect::<Vec<_>>();
    if value.is_empty()
        || value.len() > 255
        || fields.len() != 7
        || fields.iter().any(|field| {
            field.is_empty()
                || !field.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'*' | b',' | b'-' | b'/' | b'?')
                })
        })
    {
        return Err(
            "Automation schedule must use one canonical seven-field cron expression".into(),
        );
    }
    Ok(())
}

pub(super) fn validate_timestamp(label: &str, value: DateTime<Utc>) -> Result<(), String> {
    if !value.timestamp_subsec_nanos().is_multiple_of(1_000_000) {
        return Err(format!("{label} must use canonical millisecond precision"));
    }
    Ok(())
}

pub(super) fn canonical_timestamp(value: DateTime<Utc>) -> DateTime<Utc> {
    value
        .with_nanosecond(value.timestamp_subsec_millis() * 1_000_000)
        .unwrap_or(value)
}

pub(super) fn timestamp_string(value: DateTime<Utc>) -> String {
    canonical_timestamp(value).to_rfc3339_opts(SecondsFormat::Millis, true)
}

pub(super) fn canonical_json<T: Serialize>(value: &T, maximum: usize) -> Result<Vec<u8>, String> {
    let value = serde_json::to_value(value)
        .map_err(|error| format!("could not serialize Automation contract: {error}"))?;
    let encoded = serde_json::to_vec(&sort_json(value))
        .map_err(|error| format!("could not encode Automation contract: {error}"))?;
    if encoded.len() > maximum {
        return Err(format!("Automation contract exceeds {maximum} bytes"));
    }
    Ok(encoded)
}

pub(super) fn json_digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn sort_json(value: JsonValue) -> JsonValue {
    match value {
        JsonValue::Array(values) => JsonValue::Array(values.into_iter().map(sort_json).collect()),
        JsonValue::Object(values) => {
            let sorted = values
                .into_iter()
                .map(|(key, value)| (key, sort_json(value)))
                .collect::<BTreeMap<_, _>>();
            let mut object = Map::new();
            for (key, value) in sorted {
                object.insert(key, value);
            }
            JsonValue::Object(object)
        }
        value => value,
    }
}

pub(super) fn acl_integer(label: &str, value: u64) -> Result<Value, String> {
    if value > MAX_SAFE_INTEGER || value > i64::MAX as u64 {
        return Err(format!("{label} exceeds the JSON-safe integer bound"));
    }
    Ok(integer(value as i64))
}

pub(super) fn required_string(block: &Block, name: &str) -> Result<String, String> {
    block
        .attributes
        .get(name)
        .ok_or_else(|| format!("Automation attribute {name} is missing"))?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("Automation attribute {name} must be a string"))
}

pub(super) fn optional_string(block: &Block, name: &str) -> Result<Option<String>, String> {
    block
        .attributes
        .get(name)
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("Automation attribute {name} must be a string"))
        })
        .transpose()
}

pub(super) fn required_uuid(block: &Block, name: &str) -> Result<Uuid, String> {
    let value = required_string(block, name)?;
    Uuid::parse_str(&value).map_err(|error| format!("Automation {name} is invalid: {error}"))
}

pub(super) fn optional_uuid(block: &Block, name: &str) -> Result<Option<Uuid>, String> {
    optional_string(block, name)?
        .map(|value| {
            Uuid::parse_str(&value)
                .map_err(|error| format!("Automation {name} is invalid: {error}"))
        })
        .transpose()
}

pub(super) fn required_digest(block: &Block, name: &str) -> Result<String, String> {
    let value = required_string(block, name)?;
    validate_digest(&format!("Automation {name}"), &value)?;
    Ok(value)
}

pub(super) fn optional_digest(block: &Block, name: &str) -> Result<Option<String>, String> {
    let value = optional_string(block, name)?;
    value
        .map(|value| {
            validate_digest(&format!("Automation {name}"), &value)?;
            Ok(value)
        })
        .transpose()
}

pub(super) fn required_u64(block: &Block, name: &str) -> Result<u64, String> {
    let value = block
        .attributes
        .get(name)
        .ok_or_else(|| format!("Automation attribute {name} is missing"))?
        .as_number()
        .ok_or_else(|| format!("Automation attribute {name} must be an integer"))?;
    if !value.is_finite() || value < 0.0 || value.fract() != 0.0 || value > MAX_SAFE_INTEGER as f64
    {
        return Err(format!(
            "Automation attribute {name} must be a JSON-safe integer"
        ));
    }
    Ok(value as u64)
}

pub(super) fn required_string_list(block: &Block, name: &str) -> Result<Vec<String>, String> {
    let Some(Value::List(values)) = block.attributes.get(name) else {
        return Err(format!("Automation attribute {name} must be a string list"));
    };
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("Automation list {name} must contain only strings"))
        })
        .collect()
}

pub(super) fn exact_shape(
    block: &Block,
    expected_name: &str,
    attributes: &[&str],
    nested: &[&str],
) -> Result<(), String> {
    if block.name != expected_name
        || !block.labels.is_empty()
        || block.attributes.len() != attributes.len()
        || block
            .attributes
            .keys()
            .any(|key| !attributes.contains(&key.as_str()))
        || block
            .blocks
            .iter()
            .any(|child| !nested.contains(&child.name.as_str()))
    {
        return Err(format!("Automation {expected_name} block shape is invalid"));
    }
    Ok(())
}

pub(super) fn one_child<'a>(block: &'a Block, name: &str) -> Result<&'a Block, String> {
    let mut matches = block.blocks.iter().filter(|child| child.name == name);
    let child = matches
        .next()
        .ok_or_else(|| format!("Automation {name} block is missing"))?;
    if matches.next().is_some() {
        return Err(format!("Automation {name} block is duplicated"));
    }
    Ok(child)
}

pub(super) fn sorted_unique(values: &mut [String], label: &str) -> Result<(), String> {
    values.sort_unstable();
    if values.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(format!("{label} contains duplicates"));
    }
    Ok(())
}

pub(super) fn template_placeholders(value: &str) -> Result<BTreeSet<String>, String> {
    validate_single_line("Automation deduplication template", value, 512)?;
    let mut placeholders = BTreeSet::new();
    let mut cursor = 0;
    while let Some(relative_start) = value[cursor..].find('{') {
        let start = cursor + relative_start;
        if value[cursor..start].contains('}') {
            return Err("Automation deduplication template has an unmatched brace".into());
        }
        let relative_end = value[start + 1..]
            .find('}')
            .ok_or_else(|| "Automation deduplication template has an unmatched brace".to_owned())?;
        let end = start + 1 + relative_end;
        let placeholder = &value[start + 1..end];
        if !matches!(
            placeholder,
            "automation_id" | "revision_id" | "subscription_id" | "event_id" | "scheduled_at"
        ) {
            return Err(format!(
                "Automation deduplication template uses unsupported placeholder {{{placeholder}}}"
            ));
        }
        if !placeholders.insert(placeholder.to_owned()) {
            return Err(format!(
                "Automation deduplication template repeats {{{placeholder}}}"
            ));
        }
        cursor = end + 1;
    }
    if value[cursor..].contains('}') {
        return Err("Automation deduplication template has an unmatched brace".into());
    }
    Ok(placeholders)
}

// `DateTime::with_nanosecond` lives on `Timelike`; keeping the import local
// avoids exposing that trait through the public contract module.
use chrono::Timelike;
