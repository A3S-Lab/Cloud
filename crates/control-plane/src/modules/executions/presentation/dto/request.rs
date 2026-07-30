use crate::modules::executions::domain::{
    ExecutionArtifact, ExecutionProcess, ExecutionResources, ExecutionTemplate,
};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateExecutionRequest {
    pub artifact: ExecutionArtifactRequest,
    pub process: ExecutionProcessRequest,
    #[serde(default)]
    pub input: serde_json::Value,
    pub resources: ExecutionResourcesRequest,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutionArtifactRequest {
    pub uri: String,
    pub digest: String,
    pub media_type: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutionProcessRequest {
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
pub struct ExecutionResourcesRequest {
    pub cpu_millis: u64,
    pub memory_bytes: u64,
    pub pids: u32,
    pub ephemeral_storage_bytes: Option<u64>,
    pub timeout_ms: u64,
}

impl From<CreateExecutionRequest> for ExecutionTemplate {
    fn from(request: CreateExecutionRequest) -> Self {
        Self {
            artifact: ExecutionArtifact {
                uri: request.artifact.uri,
                digest: request.artifact.digest,
                media_type: request.artifact.media_type,
            },
            process: ExecutionProcess {
                command: request.process.command,
                args: request.process.args,
                working_directory: request.process.working_directory,
                environment: request.process.environment,
            },
            input: request.input,
            resources: ExecutionResources {
                cpu_millis: request.resources.cpu_millis,
                memory_bytes: request.resources.memory_bytes,
                pids: request.resources.pids,
                ephemeral_storage_bytes: request.resources.ephemeral_storage_bytes,
                timeout_ms: request.resources.timeout_ms,
            },
        }
    }
}
