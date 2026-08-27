use super::workload_profile::MAX_WORKLOAD_PROFILE_SAFE_INTEGER;
use crate::modules::shared_kernel::domain::SecretId;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_WORKLOAD_PROCESS_COMMANDS: usize = 64;
pub const MAX_WORKLOAD_PROCESS_ARGUMENTS: usize = 256;
pub const MAX_WORKLOAD_PROCESS_VALUE_BYTES: usize = 4 * 1024;
pub const MAX_WORKLOAD_ENVIRONMENT_VARIABLES: usize = 256;
pub const MAX_WORKLOAD_ENVIRONMENT_VARIABLE_NAME_BYTES: usize = 255;
pub const MAX_WORKLOAD_SERVICE_ENVIRONMENT_VALUE_BYTES: usize = 64 * 1024;
pub const MAX_WORKLOAD_EXECUTION_ENVIRONMENT_VALUE_BYTES: usize = 32 * 1024;
pub const MAX_WORKLOAD_SECRET_BINDINGS: usize = 128;
pub const MAX_WORKLOAD_SECRET_NAME_BYTES: usize = 63;
pub const MAX_WORKLOAD_SERVICE_PORTS: usize = 64;
pub const MAX_WORKLOAD_SERVICE_PORT_NAME_BYTES: usize = 63;
pub const MAX_WORKLOAD_HEALTH_PATH_BYTES: usize = 2_048;
pub const MAX_WORKLOAD_RESOURCE_CPU_MILLIS: u64 = 1_000_000;
pub const MAX_WORKLOAD_RESOURCE_MEMORY_BYTES: u64 = 1024 * 1024 * 1024 * 1024;
pub const MAX_WORKLOAD_RESOURCE_PIDS: u32 = 1_000_000;
pub const MAX_WORKLOAD_RESOURCE_EPHEMERAL_STORAGE_BYTES: u64 = 1024 * 1024 * 1024 * 1024;
pub const MAX_WORKLOAD_PROFILE_EXECUTION_TIMEOUT_MS: u64 = 900_000;

/// Developer Workflows-owned process intent.
///
/// This is deliberately not a Workloads `ServiceProcess` or an Executions
/// `ExecutionProcess`. The accepted review contract remains stable while each
/// owner is free to evolve its admission model. Application adapters perform
/// the exact translation and owner validation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkloadProcess {
    pub command: Vec<String>,
    pub args: Vec<String>,
    pub working_directory: Option<String>,
    pub environment: BTreeMap<String, String>,
}

impl WorkloadProcess {
    pub(super) fn validate_for_service(&self) -> Result<(), String> {
        self.validate_common(MAX_WORKLOAD_SERVICE_ENVIRONMENT_VALUE_BYTES)?;
        if self
            .working_directory
            .as_ref()
            .is_some_and(|value| !valid_single_line(value, MAX_WORKLOAD_PROCESS_VALUE_BYTES))
        {
            return Err("workload profile Service working directory is invalid".into());
        }
        Ok(())
    }

    pub(super) fn validate_for_execution(&self) -> Result<(), String> {
        self.validate_common(MAX_WORKLOAD_EXECUTION_ENVIRONMENT_VALUE_BYTES)?;
        if self
            .working_directory
            .as_ref()
            .is_some_and(|value| !valid_absolute_path(value))
            || self
                .environment
                .keys()
                .any(|name| name.starts_with("A3S_EXECUTION_"))
        {
            return Err("workload profile Execution process is invalid".into());
        }
        Ok(())
    }

    fn validate_common(&self, maximum_environment_value_bytes: usize) -> Result<(), String> {
        validate_string_list(
            "workload profile process command",
            &self.command,
            MAX_WORKLOAD_PROCESS_COMMANDS,
            MAX_WORKLOAD_PROCESS_VALUE_BYTES,
        )?;
        validate_string_list(
            "workload profile process argument",
            &self.args,
            MAX_WORKLOAD_PROCESS_ARGUMENTS,
            MAX_WORKLOAD_PROCESS_VALUE_BYTES,
        )?;
        if self.environment.len() > MAX_WORKLOAD_ENVIRONMENT_VARIABLES
            || self.environment.iter().any(|(name, value)| {
                !valid_environment_key(name)
                    || value.len() > maximum_environment_value_bytes
                    || value.contains('\0')
            })
        {
            return Err("workload profile process environment is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkloadSecretTarget {
    Environment { variable: String },
    File { path: String, mode: u32 },
    RegistryCredential,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkloadSecretBinding {
    pub name: String,
    pub secret_id: SecretId,
    pub version: u64,
    pub target: WorkloadSecretTarget,
}

impl WorkloadSecretBinding {
    fn validate(&self) -> Result<(), String> {
        if !valid_secret_name(&self.name)
            || self.secret_id.as_uuid().is_nil()
            || self.version == 0
            || self.version > MAX_WORKLOAD_PROFILE_SAFE_INTEGER
        {
            return Err("workload profile Secret binding identity is invalid".into());
        }
        match &self.target {
            WorkloadSecretTarget::Environment { variable } => {
                if !valid_environment_key(variable) {
                    return Err("workload profile Secret environment target is invalid".into());
                }
            }
            WorkloadSecretTarget::File { path, mode } => {
                if !valid_absolute_path(path) || *mode == 0 || *mode > 0o777 {
                    return Err("workload profile Secret file target is invalid".into());
                }
            }
            WorkloadSecretTarget::RegistryCredential => {}
        }
        Ok(())
    }

    fn target_key(&self) -> String {
        match &self.target {
            WorkloadSecretTarget::Environment { variable } => format!("environment:{variable}"),
            WorkloadSecretTarget::File { path, .. } => format!("file:{path}"),
            WorkloadSecretTarget::RegistryCredential => "registry_credential".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkloadProfileResources {
    pub cpu_millis: u64,
    pub memory_bytes: u64,
    pub pids: u32,
    pub ephemeral_storage_bytes: Option<u64>,
    pub execution_timeout_ms: Option<u64>,
}

impl WorkloadProfileResources {
    pub(super) fn validate_common(&self) -> Result<(), String> {
        if self.cpu_millis == 0
            || self.cpu_millis > MAX_WORKLOAD_RESOURCE_CPU_MILLIS
            || self.memory_bytes == 0
            || self.memory_bytes > MAX_WORKLOAD_RESOURCE_MEMORY_BYTES
            || self.pids == 0
            || self.pids > MAX_WORKLOAD_RESOURCE_PIDS
            || self.ephemeral_storage_bytes == Some(0)
            || self
                .ephemeral_storage_bytes
                .is_some_and(|value| value > MAX_WORKLOAD_RESOURCE_EPHEMERAL_STORAGE_BYTES)
        {
            return Err("workload profile resources are outside the closed bounds".into());
        }
        Ok(())
    }

    pub(super) fn validate_execution_timeout(&self) -> Result<(), String> {
        if self.execution_timeout_ms.is_none_or(|timeout| {
            timeout == 0 || timeout > MAX_WORKLOAD_PROFILE_EXECUTION_TIMEOUT_MS
        }) {
            return Err("scheduled Task execution timeout is outside the closed bounds".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkloadServicePort {
    pub name: String,
    pub container_port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkloadHttpHealthCheck {
    pub port_name: String,
    pub path: String,
    pub interval_ms: u64,
    pub timeout_ms: u64,
    pub healthy_threshold: u16,
    pub unhealthy_threshold: u16,
    pub stabilization_window_ms: u64,
}

pub(super) fn validate_service_intent(
    process: &WorkloadProcess,
    secrets: &[WorkloadSecretBinding],
    resources: &WorkloadProfileResources,
    ports: &[WorkloadServicePort],
    health: Option<&WorkloadHttpHealthCheck>,
) -> Result<(), String> {
    process.validate_for_service()?;
    resources.validate_common()?;
    if secrets.len() > MAX_WORKLOAD_SECRET_BINDINGS || ports.len() > MAX_WORKLOAD_SERVICE_PORTS {
        return Err("workload profile Service shape exceeds its closed bounds".into());
    }

    let mut secret_names = BTreeSet::new();
    let mut secret_targets = BTreeSet::new();
    for secret in secrets {
        secret.validate()?;
        if !secret_names.insert(&secret.name) || !secret_targets.insert(secret.target_key()) {
            return Err("workload profile Secret names and targets must be unique".into());
        }
        if matches!(
            &secret.target,
            WorkloadSecretTarget::Environment { variable }
                if process.environment.contains_key(variable)
        ) {
            return Err("workload profile environment and Secret targets overlap".into());
        }
    }

    let mut port_names = BTreeSet::new();
    for port in ports {
        if !valid_port_name(&port.name)
            || port.container_port == 0
            || !port_names.insert(&port.name)
        {
            return Err("workload profile Service ports are invalid".into());
        }
    }
    if let Some(health) = health {
        if !port_names.contains(&health.port_name)
            || !health.path.starts_with('/')
            || health.path.len() > MAX_WORKLOAD_HEALTH_PATH_BYTES
            || health.path.contains(['\0', '\r', '\n'])
            || health.interval_ms == 0
            || health.interval_ms > MAX_WORKLOAD_PROFILE_SAFE_INTEGER
            || health.timeout_ms == 0
            || health.timeout_ms > MAX_WORKLOAD_PROFILE_SAFE_INTEGER
            || health.timeout_ms > health.interval_ms
            || health.healthy_threshold == 0
            || health.unhealthy_threshold == 0
            || health.stabilization_window_ms == 0
            || health.stabilization_window_ms > MAX_WORKLOAD_PROFILE_SAFE_INTEGER
        {
            return Err("workload profile HTTP health check is invalid".into());
        }
    }
    Ok(())
}

pub(super) fn validate_execution_intent(
    process: &WorkloadProcess,
    resources: &WorkloadProfileResources,
) -> Result<(), String> {
    process.validate_for_execution()?;
    resources.validate_common()?;
    resources.validate_execution_timeout()
}

fn validate_string_list(
    label: &str,
    values: &[String],
    maximum_items: usize,
    maximum_bytes: usize,
) -> Result<(), String> {
    if values.len() > maximum_items
        || values
            .iter()
            .any(|value| value.len() > maximum_bytes || value.contains('\0'))
    {
        return Err(format!("{label} list is invalid"));
    }
    Ok(())
}

fn valid_single_line(value: &str, maximum_bytes: usize) -> bool {
    !value.trim().is_empty() && value.len() <= maximum_bytes && !value.contains(['\0', '\r', '\n'])
}

fn valid_environment_key(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_WORKLOAD_ENVIRONMENT_VARIABLE_NAME_BYTES
        && value.bytes().enumerate().all(|(index, byte)| {
            byte == b'_' || byte.is_ascii_uppercase() || index > 0 && byte.is_ascii_digit()
        })
}

fn valid_secret_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_WORKLOAD_SECRET_NAME_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn valid_port_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_WORKLOAD_SERVICE_PORT_NAME_BYTES
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_' | b'.')
        })
}

fn valid_absolute_path(value: &str) -> bool {
    value.starts_with('/')
        && value.len() <= MAX_WORKLOAD_PROCESS_VALUE_BYTES
        && !value.contains(['\0', '\r', '\n'])
        && !value.contains("//")
        && value
            .split('/')
            .skip(1)
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}
