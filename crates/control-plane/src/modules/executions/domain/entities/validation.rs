pub(super) fn valid_sha256(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

pub(super) fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && !value.contains(['\0', '\r', '\n'])
        && !value
            .bytes()
            .any(|byte| !(byte.is_ascii_alphanumeric() || b"-_ .".contains(&byte)))
        && !value.starts_with(['-', '_', '.', ' '])
        && !value.ends_with(['-', '_', '.', ' '])
}

pub(super) fn valid_absolute_path(value: &str) -> bool {
    value.starts_with('/')
        && value.len() <= 4096
        && !value.contains(['\0', '\r', '\n'])
        && !value.split('/').any(|segment| segment == "..")
}

pub(super) fn valid_environment_name(value: &str) -> bool {
    let mut bytes = value.bytes();
    bytes.next().is_some_and(|first| {
        (first.is_ascii_alphabetic() || first == b'_')
            && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            && value.len() <= 255
    })
}
