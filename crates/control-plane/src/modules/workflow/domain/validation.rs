use a3s_acl::{Block, Value};

pub(super) fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 96
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

pub(super) fn validate_identifier(label: &str, value: &str) -> Result<(), String> {
    if valid_identifier(value) {
        Ok(())
    } else {
        Err(format!(
            "{label} must use 1-96 ASCII letters, numbers, hyphens, or underscores"
        ))
    }
}

pub(super) fn validate_revision(label: &str, value: &str) -> Result<(), String> {
    if !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'+'))
    {
        Ok(())
    } else {
        Err(format!(
            "{label} must use 1-128 portable revision characters"
        ))
    }
}

pub(super) fn validate_text(
    label: &str,
    value: &str,
    minimum: usize,
    maximum: usize,
) -> Result<(), String> {
    let length = value.trim().chars().count();
    if !(minimum..=maximum).contains(&length) || value.contains('\0') {
        return Err(format!(
            "{label} must contain between {minimum} and {maximum} characters"
        ));
    }
    Ok(())
}

pub(super) fn required_value<'a>(block: &'a Block, name: &str) -> Result<&'a Value, String> {
    block
        .attributes
        .get(name)
        .ok_or_else(|| format!("Workflow ACL field {name:?} is required"))
}

pub(super) fn required_string(block: &Block, name: &str) -> Result<String, String> {
    required_value(block, name)?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("Workflow ACL field {name:?} must be a string"))
}

pub(super) fn optional_string(block: &Block, name: &str) -> Result<Option<String>, String> {
    block
        .attributes
        .get(name)
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("Workflow ACL field {name:?} must be a string"))
        })
        .transpose()
}

pub(super) fn required_strings(block: &Block, name: &str) -> Result<Vec<String>, String> {
    let Value::List(values) = required_value(block, name)? else {
        return Err(format!("Workflow ACL field {name:?} must be a string list"));
    };
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("Workflow ACL field {name:?} must be a string list"))
        })
        .collect()
}
