use serde::Serialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
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

pub(super) fn validate_media_type(label: &str, value: &str) -> Result<(), String> {
    let mut segments = value.split('/');
    let Some(top_level) = segments.next() else {
        return Err(format!("{label} is invalid"));
    };
    let Some(subtype) = segments.next() else {
        return Err(format!("{label} is invalid"));
    };
    if segments.next().is_some()
        || !valid_media_type_segment(top_level)
        || !valid_media_type_segment(subtype)
        || value.len() > 127
    {
        return Err(format!(
            "{label} must be one canonical lowercase media type without parameters"
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

pub(super) fn validate_dotted_identifier(
    label: &str,
    value: &str,
    maximum_length: usize,
) -> Result<(), String> {
    if value.is_empty()
        || value.len() > maximum_length
        || value.split('.').any(|segment| {
            segment.is_empty()
                || segment.starts_with('-')
                || segment.ends_with('-')
                || !segment
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        })
    {
        Err(format!("{label} must use portable dotted lowercase syntax"))
    } else {
        Ok(())
    }
}

pub(super) fn validate_portable_name(
    label: &str,
    value: &str,
    maximum_length: usize,
) -> Result<(), String> {
    if value.is_empty()
        || value.len() > maximum_length
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        Err(format!("{label} is invalid"))
    } else {
        Ok(())
    }
}

pub(super) fn validate_single_line(
    label: &str,
    value: &str,
    maximum_length: usize,
) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > maximum_length
        || value.contains('\0')
        || value.contains(['\r', '\n'])
    {
        Err(format!(
            "{label} must be a bounded nonempty single-line value"
        ))
    } else {
        Ok(())
    }
}

pub(super) fn validate_object_key(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 2_048
        || value.starts_with('/')
        || value.ends_with('/')
        || value.contains(['\0', '\r', '\n', '\\'])
        || value
            .split('/')
            .any(|segment| segment.is_empty() || matches!(segment, "." | ".."))
    {
        Err("Function immutable-object key is invalid".into())
    } else {
        Ok(())
    }
}

pub(super) fn canonical_json(value: &impl Serialize, maximum: usize) -> Result<Vec<u8>, String> {
    let value = serde_json::to_value(value)
        .map_err(|error| format!("could not project Function contract: {error}"))?;
    let encoded = serde_json::to_vec(&sort_json(value))
        .map_err(|error| format!("could not encode Function contract: {error}"))?;
    if encoded.len() > maximum {
        return Err(format!("Function contract exceeds {maximum} bytes"));
    }
    Ok(encoded)
}

pub(super) fn json_digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn sort_json(value: Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.into_iter().map(sort_json).collect()),
        Value::Object(values) => {
            let sorted = values
                .into_iter()
                .map(|(key, value)| (key, sort_json(value)))
                .collect::<BTreeMap<_, _>>();
            let mut object = Map::new();
            for (key, value) in sorted {
                object.insert(key, value);
            }
            Value::Object(object)
        }
        value => value,
    }
}
