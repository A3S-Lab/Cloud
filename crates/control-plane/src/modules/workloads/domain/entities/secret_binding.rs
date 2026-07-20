use crate::modules::shared_kernel::domain::SecretId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SecretBindingTarget {
    Environment { variable: String },
    File { path: String, mode: u32 },
    RegistryCredential,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretBinding {
    pub name: String,
    pub secret_id: SecretId,
    pub version: u64,
    pub target: SecretBindingTarget,
}

impl SecretBinding {
    pub fn validate(&self) -> Result<(), String> {
        if !valid_name(&self.name) || self.secret_id.as_uuid().is_nil() || self.version == 0 {
            return Err("Secret binding identity is invalid".into());
        }
        match &self.target {
            SecretBindingTarget::Environment { variable } => {
                if !valid_environment_key(variable) {
                    return Err("Secret environment target is invalid".into());
                }
            }
            SecretBindingTarget::File { path, mode } => {
                if !valid_absolute_path(path) || *mode == 0 || *mode > 0o777 {
                    return Err("Secret file target is invalid".into());
                }
            }
            SecretBindingTarget::RegistryCredential => {}
        }
        Ok(())
    }

    pub fn target_key(&self) -> String {
        match &self.target {
            SecretBindingTarget::Environment { variable } => format!("environment:{variable}"),
            SecretBindingTarget::File { path, .. } => format!("file:{path}"),
            SecretBindingTarget::RegistryCredential => "registry_credential".into(),
        }
    }
}

fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 63
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn valid_environment_key(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && value.bytes().enumerate().all(|(index, byte)| {
            byte == b'_' || byte.is_ascii_uppercase() || index > 0 && byte.is_ascii_digit()
        })
}

fn valid_absolute_path(value: &str) -> bool {
    value.starts_with('/')
        && value.len() <= 4096
        && !value.contains(['\0', '\r', '\n'])
        && !value.contains("//")
        && value
            .split('/')
            .skip(1)
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_typed_environment_file_and_registry_targets() {
        let secret_id = SecretId::new();
        SecretBinding {
            name: "database-password".into(),
            secret_id,
            version: 3,
            target: SecretBindingTarget::Environment {
                variable: "DATABASE_PASSWORD".into(),
            },
        }
        .validate()
        .expect("environment binding");
        SecretBinding {
            name: "tls-key".into(),
            secret_id,
            version: 2,
            target: SecretBindingTarget::File {
                path: "/run/secrets/tls.key".into(),
                mode: 0o400,
            },
        }
        .validate()
        .expect("file binding");
        SecretBinding {
            name: "registry".into(),
            secret_id,
            version: 4,
            target: SecretBindingTarget::RegistryCredential,
        }
        .validate()
        .expect("registry credential binding");
        assert!(SecretBinding {
            name: "escape".into(),
            secret_id,
            version: 1,
            target: SecretBindingTarget::File {
                path: "/run/../host".into(),
                mode: 0o600,
            },
        }
        .validate()
        .is_err());
    }
}
