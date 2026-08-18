use std::net::SocketAddr;

/// Runtime-owned values embedded in every managed Gateway snapshot.
///
/// These paths belong to the target Gateway process, not to the control-plane
/// host. Validation is therefore lexical and target-neutral: both POSIX and
/// Windows absolute paths are accepted regardless of the host compiling the
/// snapshot.
#[derive(Debug, Clone, Copy)]
pub(crate) struct GatewaySnapshotRuntimeSettings<'a> {
    pub entrypoint_address: &'a str,
    pub management_address: &'a str,
    pub management_path_prefix: &'a str,
    pub management_auth_token_env: &'a str,
    pub upstream_request_timeout_ms: u64,
    pub certificate_directory: &'a str,
    pub managed_state_file: &'a str,
}

impl GatewaySnapshotRuntimeSettings<'_> {
    pub(crate) fn validate(self) -> Result<(), String> {
        let entrypoint = self
            .entrypoint_address
            .parse::<SocketAddr>()
            .map_err(|error| format!("invalid Gateway entrypoint address: {error}"))?;
        let management = self
            .management_address
            .parse::<SocketAddr>()
            .map_err(|error| format!("invalid Gateway management address: {error}"))?;
        if entrypoint.port() == 0
            || management.port() == 0
            || !management.ip().is_loopback()
            || !valid_path_prefix(self.management_path_prefix)
            || !valid_environment_name(self.management_auth_token_env)
            || self.upstream_request_timeout_ms == 0
            || self.upstream_request_timeout_ms > 3_600_000
            || !valid_runtime_directory(self.certificate_directory)
            || !valid_runtime_file(self.managed_state_file)
        {
            return Err("Gateway snapshot compiler configuration is invalid".into());
        }
        Ok(())
    }
}

fn valid_path_prefix(value: &str) -> bool {
    value.starts_with('/') && value.len() <= 255 && !value.contains(['\0', '\r', '\n', '?', '#'])
}

fn valid_environment_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && value.bytes().enumerate().all(|(index, byte)| {
            byte == b'_' || byte.is_ascii_uppercase() || index > 0 && byte.is_ascii_digit()
        })
}

fn valid_runtime_directory(value: &str) -> bool {
    valid_runtime_path(value) && absolute_path_kind(value).is_some()
}

fn valid_runtime_file(value: &str) -> bool {
    if !valid_runtime_path(value) || value.ends_with(['/', '\\']) {
        return false;
    }
    let components = path_components(value);
    match absolute_path_kind(value) {
        Some(AbsolutePathKind::Posix) => !components.is_empty(),
        Some(AbsolutePathKind::Drive) => components.len() >= 2,
        Some(AbsolutePathKind::Unc) => components.len() >= 3,
        None => false,
    }
}

fn valid_runtime_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 4096
        && !value.contains(['\0', '\r', '\n'])
        && !value
            .split(['/', '\\'])
            .any(|component| matches!(component, "." | ".."))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AbsolutePathKind {
    Posix,
    Drive,
    Unc,
}

fn absolute_path_kind(value: &str) -> Option<AbsolutePathKind> {
    let bytes = value.as_bytes();
    if value.starts_with('/') {
        return Some(AbsolutePathKind::Posix);
    }
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\')
    {
        return Some(AbsolutePathKind::Drive);
    }
    if value.starts_with("\\\\") {
        let components = path_components(value);
        if components.len() >= 2 && components[0] != "." && components[0] != "?" {
            return Some(AbsolutePathKind::Unc);
        }
    }
    None
}

fn path_components(value: &str) -> Vec<&str> {
    value
        .split(['/', '\\'])
        .filter(|component| !component.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings<'a>(directory: &'a str, file: &'a str) -> GatewaySnapshotRuntimeSettings<'a> {
        GatewaySnapshotRuntimeSettings {
            entrypoint_address: "0.0.0.0:8081",
            management_address: "127.0.0.1:9090",
            management_path_prefix: "/api/gateway",
            management_auth_token_env: "A3S_GATEWAY_ADMIN_TOKEN",
            upstream_request_timeout_ms: 30_000,
            certificate_directory: directory,
            managed_state_file: file,
        }
    }

    #[test]
    fn target_paths_are_validated_independently_of_the_compiler_host() {
        for (directory, file) in [
            (
                "/var/lib/a3s-cloud/gateway/certificates",
                "/var/lib/a3s-gateway/managed-snapshot.json",
            ),
            (
                r"C:\ProgramData\A3S\gateway\certificates",
                r"C:\ProgramData\A3S\gateway\managed-snapshot.json",
            ),
            (
                r"\\gateway-host\a3s\certificates",
                r"\\gateway-host\a3s\managed-snapshot.json",
            ),
            (
                "/var/lib/a3s-cloud/gateway/certificates/",
                "/var/lib/a3s-gateway/managed-snapshot.json",
            ),
        ] {
            settings(directory, file)
                .validate()
                .expect("portable target path");
        }
    }

    #[test]
    fn relative_parent_and_directory_shaped_file_paths_are_rejected() {
        for (directory, file) in [
            ("relative/certificates", "/var/lib/a3s-gateway/state.json"),
            (
                "/var/lib/../certificates",
                "/var/lib/a3s-gateway/state.json",
            ),
            ("/var/lib/certificates", "relative/state.json"),
            ("/var/lib/certificates", "/"),
            (r"C:\certificates", r"C:\"),
            (r"\\host\share", r"\\host\share"),
        ] {
            assert!(
                settings(directory, file).validate().is_err(),
                "accepted directory={directory:?}, file={file:?}"
            );
        }
    }
}
