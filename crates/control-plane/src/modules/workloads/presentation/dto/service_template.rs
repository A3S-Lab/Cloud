use crate::modules::workloads::domain::entities::{
    HttpHealthCheck, OciArtifactReference, RequestedServiceTemplate, SecretBinding,
    SecretBindingTarget, ServicePort, ServiceProcess, ServiceResources,
};
use crate::modules::workloads::SourceWorkloadTemplate;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ServiceTemplateDto {
    pub artifact: OciArtifactReferenceDto,
    #[serde(default)]
    pub process: ServiceProcessDto,
    #[serde(default)]
    pub secrets: Vec<SecretBindingDto>,
    pub resources: ServiceResourcesDto,
    pub ports: Vec<ServicePortDto>,
    pub health: HttpHealthCheckDto,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceWorkloadTemplateDto {
    #[serde(default)]
    pub process: ServiceProcessDto,
    #[serde(default)]
    pub secrets: Vec<SecretBindingDto>,
    pub resources: ServiceResourcesDto,
    pub ports: Vec<ServicePortDto>,
    pub health: HttpHealthCheckDto,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SecretBindingDto {
    pub name: String,
    pub secret_id: Uuid,
    pub version: u64,
    pub target: SecretBindingTargetDto,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SecretBindingTargetDto {
    Environment { variable: String },
    File { path: String, mode: u32 },
    RegistryCredential,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OciArtifactReferenceDto {
    pub uri: String,
    pub expected_digest: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ServiceProcessDto {
    #[serde(default)]
    pub command: Vec<String>,
    #[serde(default)]
    pub args: Vec<String>,
    pub working_directory: Option<String>,
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ServiceResourcesDto {
    pub cpu_millis: u64,
    pub memory_bytes: u64,
    pub pids: u32,
    pub ephemeral_storage_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ServicePortDto {
    pub name: String,
    pub container_port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HttpHealthCheckDto {
    pub port_name: String,
    pub path: String,
    pub interval_ms: u64,
    pub timeout_ms: u64,
    pub healthy_threshold: u16,
    pub unhealthy_threshold: u16,
    pub stabilization_window_ms: u64,
}

impl From<ServiceTemplateDto> for RequestedServiceTemplate {
    fn from(template: ServiceTemplateDto) -> Self {
        Self {
            artifact: OciArtifactReference {
                uri: template.artifact.uri,
                expected_digest: template.artifact.expected_digest,
            },
            process: ServiceProcess {
                command: template.process.command,
                args: template.process.args,
                working_directory: template.process.working_directory,
                environment: template.process.environment,
            },
            secrets: template
                .secrets
                .into_iter()
                .map(|binding| SecretBinding {
                    name: binding.name,
                    secret_id: crate::modules::shared_kernel::domain::SecretId::from_uuid(
                        binding.secret_id,
                    ),
                    version: binding.version,
                    target: match binding.target {
                        SecretBindingTargetDto::Environment { variable } => {
                            SecretBindingTarget::Environment { variable }
                        }
                        SecretBindingTargetDto::File { path, mode } => {
                            SecretBindingTarget::File { path, mode }
                        }
                        SecretBindingTargetDto::RegistryCredential => {
                            SecretBindingTarget::RegistryCredential
                        }
                    },
                })
                .collect(),
            resources: ServiceResources {
                cpu_millis: template.resources.cpu_millis,
                memory_bytes: template.resources.memory_bytes,
                pids: template.resources.pids,
                ephemeral_storage_bytes: template.resources.ephemeral_storage_bytes,
            },
            ports: template
                .ports
                .into_iter()
                .map(|port| ServicePort {
                    name: port.name,
                    container_port: port.container_port,
                })
                .collect(),
            health: HttpHealthCheck {
                port_name: template.health.port_name,
                path: template.health.path,
                interval_ms: template.health.interval_ms,
                timeout_ms: template.health.timeout_ms,
                healthy_threshold: template.health.healthy_threshold,
                unhealthy_threshold: template.health.unhealthy_threshold,
                stabilization_window_ms: template.health.stabilization_window_ms,
            },
        }
    }
}

impl From<SourceWorkloadTemplateDto> for SourceWorkloadTemplate {
    fn from(template: SourceWorkloadTemplateDto) -> Self {
        Self {
            process: ServiceProcess {
                command: template.process.command,
                args: template.process.args,
                working_directory: template.process.working_directory,
                environment: template.process.environment,
            },
            secrets: template
                .secrets
                .into_iter()
                .map(|binding| SecretBinding {
                    name: binding.name,
                    secret_id: crate::modules::shared_kernel::domain::SecretId::from_uuid(
                        binding.secret_id,
                    ),
                    version: binding.version,
                    target: match binding.target {
                        SecretBindingTargetDto::Environment { variable } => {
                            SecretBindingTarget::Environment { variable }
                        }
                        SecretBindingTargetDto::File { path, mode } => {
                            SecretBindingTarget::File { path, mode }
                        }
                        SecretBindingTargetDto::RegistryCredential => {
                            SecretBindingTarget::RegistryCredential
                        }
                    },
                })
                .collect(),
            resources: ServiceResources {
                cpu_millis: template.resources.cpu_millis,
                memory_bytes: template.resources.memory_bytes,
                pids: template.resources.pids,
                ephemeral_storage_bytes: template.resources.ephemeral_storage_bytes,
            },
            ports: template
                .ports
                .into_iter()
                .map(|port| ServicePort {
                    name: port.name,
                    container_port: port.container_port,
                })
                .collect(),
            health: HttpHealthCheck {
                port_name: template.health.port_name,
                path: template.health.path,
                interval_ms: template.health.interval_ms,
                timeout_ms: template.health.timeout_ms,
                healthy_threshold: template.health.healthy_threshold,
                unhealthy_threshold: template.health.unhealthy_threshold,
                stabilization_window_ms: template.health.stabilization_window_ms,
            },
        }
    }
}

impl From<RequestedServiceTemplate> for ServiceTemplateDto {
    fn from(template: RequestedServiceTemplate) -> Self {
        Self {
            artifact: OciArtifactReferenceDto {
                uri: template.artifact.uri,
                expected_digest: template.artifact.expected_digest,
            },
            process: ServiceProcessDto {
                command: template.process.command,
                args: template.process.args,
                working_directory: template.process.working_directory,
                environment: template.process.environment,
            },
            secrets: template
                .secrets
                .into_iter()
                .map(|binding| SecretBindingDto {
                    name: binding.name,
                    secret_id: binding.secret_id.as_uuid(),
                    version: binding.version,
                    target: match binding.target {
                        SecretBindingTarget::Environment { variable } => {
                            SecretBindingTargetDto::Environment { variable }
                        }
                        SecretBindingTarget::File { path, mode } => {
                            SecretBindingTargetDto::File { path, mode }
                        }
                        SecretBindingTarget::RegistryCredential => {
                            SecretBindingTargetDto::RegistryCredential
                        }
                    },
                })
                .collect(),
            resources: ServiceResourcesDto {
                cpu_millis: template.resources.cpu_millis,
                memory_bytes: template.resources.memory_bytes,
                pids: template.resources.pids,
                ephemeral_storage_bytes: template.resources.ephemeral_storage_bytes,
            },
            ports: template
                .ports
                .into_iter()
                .map(|port| ServicePortDto {
                    name: port.name,
                    container_port: port.container_port,
                })
                .collect(),
            health: HttpHealthCheckDto {
                port_name: template.health.port_name,
                path: template.health.path,
                interval_ms: template.health.interval_ms,
                timeout_ms: template.health.timeout_ms,
                healthy_threshold: template.health.healthy_threshold,
                unhealthy_threshold: template.health.unhealthy_threshold,
                stabilization_window_ms: template.health.stabilization_window_ms,
            },
        }
    }
}
