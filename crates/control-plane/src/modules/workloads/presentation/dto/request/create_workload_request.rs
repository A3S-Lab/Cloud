use crate::modules::workloads::domain::entities::{
    HttpHealthCheck, OciArtifactReference, RequestedServiceTemplate, SecretBinding,
    SecretBindingTarget, ServicePort, ServiceProcess, ServiceResources,
};
use serde::Deserialize;
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateWorkloadRequest {
    pub name: String,
    pub template: ServiceTemplateRequest,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ServiceTemplateRequest {
    pub artifact: OciArtifactRequest,
    #[serde(default)]
    pub process: ServiceProcessRequest,
    #[serde(default)]
    pub secrets: Vec<SecretBindingRequest>,
    pub resources: ServiceResourcesRequest,
    pub ports: Vec<ServicePortRequest>,
    pub health: HttpHealthCheckRequest,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SecretBindingRequest {
    pub name: String,
    pub secret_id: Uuid,
    pub version: u64,
    pub target: SecretBindingTargetRequest,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SecretBindingTargetRequest {
    Environment { variable: String },
    File { path: String, mode: u32 },
    RegistryCredential,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OciArtifactRequest {
    pub uri: String,
    pub expected_digest: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ServiceProcessRequest {
    #[serde(default)]
    pub command: Vec<String>,
    #[serde(default)]
    pub args: Vec<String>,
    pub working_directory: Option<String>,
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ServiceResourcesRequest {
    pub cpu_millis: u64,
    pub memory_bytes: u64,
    pub pids: u32,
    pub ephemeral_storage_bytes: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ServicePortRequest {
    pub name: String,
    pub container_port: u16,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HttpHealthCheckRequest {
    pub port_name: String,
    pub path: String,
    pub interval_ms: u64,
    pub timeout_ms: u64,
    pub healthy_threshold: u16,
    pub unhealthy_threshold: u16,
    pub stabilization_window_ms: u64,
}

impl ServiceTemplateRequest {
    pub fn into_domain(self) -> RequestedServiceTemplate {
        RequestedServiceTemplate {
            artifact: OciArtifactReference {
                uri: self.artifact.uri,
                expected_digest: self.artifact.expected_digest,
            },
            process: ServiceProcess {
                command: self.process.command,
                args: self.process.args,
                working_directory: self.process.working_directory,
                environment: self.process.environment,
            },
            secrets: self
                .secrets
                .into_iter()
                .map(|binding| SecretBinding {
                    name: binding.name,
                    secret_id: crate::modules::shared_kernel::domain::SecretId::from_uuid(
                        binding.secret_id,
                    ),
                    version: binding.version,
                    target: match binding.target {
                        SecretBindingTargetRequest::Environment { variable } => {
                            SecretBindingTarget::Environment { variable }
                        }
                        SecretBindingTargetRequest::File { path, mode } => {
                            SecretBindingTarget::File { path, mode }
                        }
                        SecretBindingTargetRequest::RegistryCredential => {
                            SecretBindingTarget::RegistryCredential
                        }
                    },
                })
                .collect(),
            resources: ServiceResources {
                cpu_millis: self.resources.cpu_millis,
                memory_bytes: self.resources.memory_bytes,
                pids: self.resources.pids,
                ephemeral_storage_bytes: self.resources.ephemeral_storage_bytes,
            },
            ports: self
                .ports
                .into_iter()
                .map(|port| ServicePort {
                    name: port.name,
                    container_port: port.container_port,
                })
                .collect(),
            health: HttpHealthCheck {
                port_name: self.health.port_name,
                path: self.health.path,
                interval_ms: self.health.interval_ms,
                timeout_ms: self.health.timeout_ms,
                healthy_threshold: self.health.healthy_threshold,
                unhealthy_threshold: self.health.unhealthy_threshold,
                stabilization_window_ms: self.health.stabilization_window_ms,
            },
        }
    }
}
