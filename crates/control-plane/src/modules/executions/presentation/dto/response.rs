use crate::modules::executions::application::{
    CancelExecutionResult, CreateExecutionResult, CreateExecutionTemplateResult,
};
use crate::modules::executions::domain::{
    Execution, ExecutionArtifact, ExecutionOutcome, ExecutionProcess, ExecutionResources,
    ExecutionStatus, ExecutionTemplate, ExecutionTemplateRevision, WorkflowExecutionBinding,
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionTemplateMutationResponse {
    pub execution_template: ExecutionTemplateRevisionResponse,
    pub replayed: bool,
}

impl From<CreateExecutionTemplateResult> for ExecutionTemplateMutationResponse {
    fn from(result: CreateExecutionTemplateResult) -> Self {
        Self {
            execution_template: result.revision.into(),
            replayed: result.replayed,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionTemplateRevisionResponse {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub template_id: Uuid,
    pub revision_id: Uuid,
    pub definition_acl: String,
    pub definition_digest: String,
    pub capability: &'static str,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
}

impl From<ExecutionTemplateRevision> for ExecutionTemplateRevisionResponse {
    fn from(revision: ExecutionTemplateRevision) -> Self {
        Self {
            organization_id: revision.organization_id.as_uuid(),
            project_id: revision.project_id.as_uuid(),
            template_id: revision.template_id.as_uuid(),
            revision_id: revision.revision_id.as_uuid(),
            definition_acl: revision.definition.canonical_acl().to_owned(),
            definition_digest: revision.definition.digest().to_string(),
            capability: crate::modules::executions::domain::EXECUTION_TEMPLATE_CAPABILITY,
            created_by: revision.created_by.as_uuid(),
            created_at: revision.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionMutationResponse {
    pub execution: ExecutionResponse,
    pub replayed: bool,
}

impl From<CreateExecutionResult> for ExecutionMutationResponse {
    fn from(result: CreateExecutionResult) -> Self {
        Self {
            execution: result.execution.into(),
            replayed: result.replayed,
        }
    }
}

impl From<CancelExecutionResult> for ExecutionMutationResponse {
    fn from(result: CancelExecutionResult) -> Self {
        Self {
            execution: result.execution.into(),
            replayed: result.replayed,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionResponse {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub environment_id: Uuid,
    pub id: Uuid,
    pub operation_id: Uuid,
    pub workflow: Option<WorkflowExecutionBindingResponse>,
    pub template: ExecutionTemplateResponse,
    pub template_digest: String,
    pub status: ExecutionStatus,
    pub outcome: Option<ExecutionOutcomeResponse>,
    pub aggregate_version: u64,
    pub requested_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub cancellation_requested_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
}

impl From<Execution> for ExecutionResponse {
    fn from(execution: Execution) -> Self {
        Self {
            organization_id: execution.organization_id.as_uuid(),
            project_id: execution.project_id.as_uuid(),
            environment_id: execution.environment_id.as_uuid(),
            id: execution.id.as_uuid(),
            operation_id: execution.operation_id.as_uuid(),
            workflow: execution.workflow.map(Into::into),
            template: execution.template.into(),
            template_digest: execution.template_digest,
            status: execution.status,
            outcome: execution.outcome.map(Into::into),
            aggregate_version: execution.aggregate_version,
            requested_at: execution.requested_at,
            updated_at: execution.updated_at,
            started_at: execution.started_at,
            cancellation_requested_at: execution.cancellation_requested_at,
            finished_at: execution.finished_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowExecutionBindingResponse {
    pub workflow_run_id: Uuid,
    pub plan_revision_id: Uuid,
    pub plan_digest: String,
    pub step_id: String,
    pub step_attempt: u64,
    pub execution_template_id: Uuid,
    pub execution_template_revision_id: Uuid,
    pub execution_template_digest: String,
}

impl From<WorkflowExecutionBinding> for WorkflowExecutionBindingResponse {
    fn from(binding: WorkflowExecutionBinding) -> Self {
        Self {
            workflow_run_id: binding.workflow_run_id.as_uuid(),
            plan_revision_id: binding.plan_revision_id.as_uuid(),
            plan_digest: binding.plan_digest.to_string(),
            step_id: binding.step_id,
            step_attempt: binding.step_attempt,
            execution_template_id: binding.execution_template_id.as_uuid(),
            execution_template_revision_id: binding.execution_template_revision_id.as_uuid(),
            execution_template_digest: binding.execution_template_digest.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionTemplateResponse {
    pub artifact: ExecutionArtifactResponse,
    pub process: ExecutionProcessResponse,
    pub input: serde_json::Value,
    pub resources: ExecutionResourcesResponse,
}

impl From<ExecutionTemplate> for ExecutionTemplateResponse {
    fn from(template: ExecutionTemplate) -> Self {
        Self {
            artifact: template.artifact.into(),
            process: template.process.into(),
            input: template.input,
            resources: template.resources.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionArtifactResponse {
    pub uri: String,
    pub digest: String,
    pub media_type: String,
}

impl From<ExecutionArtifact> for ExecutionArtifactResponse {
    fn from(artifact: ExecutionArtifact) -> Self {
        Self {
            uri: artifact.uri,
            digest: artifact.digest,
            media_type: artifact.media_type,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionProcessResponse {
    pub command: Vec<String>,
    pub args: Vec<String>,
    pub working_directory: Option<String>,
    pub environment: BTreeMap<String, String>,
}

impl From<ExecutionProcess> for ExecutionProcessResponse {
    fn from(process: ExecutionProcess) -> Self {
        Self {
            command: process.command,
            args: process.args,
            working_directory: process.working_directory,
            environment: process.environment,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionResourcesResponse {
    pub cpu_millis: u64,
    pub memory_bytes: u64,
    pub pids: u32,
    pub ephemeral_storage_bytes: Option<u64>,
    pub timeout_ms: u64,
}

impl From<ExecutionResources> for ExecutionResourcesResponse {
    fn from(resources: ExecutionResources) -> Self {
        Self {
            cpu_millis: resources.cpu_millis,
            memory_bytes: resources.memory_bytes,
            pids: resources.pids,
            ephemeral_storage_bytes: resources.ephemeral_storage_bytes,
            timeout_ms: resources.timeout_ms,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExecutionOutcomeResponse {
    Succeeded {
        #[serde(rename = "exitCode")]
        exit_code: i32,
    },
    Failed {
        #[serde(rename = "exitCode")]
        exit_code: Option<i32>,
        reason: String,
    },
    Cancelled,
}

impl From<ExecutionOutcome> for ExecutionOutcomeResponse {
    fn from(outcome: ExecutionOutcome) -> Self {
        match outcome {
            ExecutionOutcome::Succeeded { exit_code } => Self::Succeeded { exit_code },
            ExecutionOutcome::Failed { exit_code, reason } => Self::Failed { exit_code, reason },
            ExecutionOutcome::Cancelled => Self::Cancelled,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::executions::domain::{
        ExecutionArtifact, ExecutionProcess, ExecutionResources,
    };
    use crate::modules::shared_kernel::domain::{
        EnvironmentId, ExecutionId, OrganizationId, ProjectId,
    };
    use std::collections::BTreeMap;

    #[test]
    fn response_uses_camel_case_and_hides_runtime_routing_identity() {
        let digest = format!("sha256:{}", "a".repeat(64));
        let execution = Execution::create(
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            ExecutionId::new(),
            ExecutionTemplate {
                artifact: ExecutionArtifact {
                    uri: format!("oci://registry.example/functions/echo@{digest}"),
                    digest,
                    media_type: "application/vnd.oci.image.manifest.v1+json".into(),
                },
                process: ExecutionProcess {
                    command: Vec::new(),
                    args: Vec::new(),
                    working_directory: None,
                    environment: BTreeMap::new(),
                },
                input: serde_json::json!({"hello": "world"}),
                resources: ExecutionResources {
                    cpu_millis: 100,
                    memory_bytes: 64 * 1024 * 1024,
                    pids: 32,
                    ephemeral_storage_bytes: None,
                    timeout_ms: 1_000,
                },
            },
            Utc::now(),
        )
        .expect("execution");
        let encoded = serde_json::to_value(ExecutionResponse::from(execution)).expect("response");
        assert!(encoded.get("organizationId").is_some());
        assert!(encoded.get("templateDigest").is_some());
        assert!(encoded["template"]["resources"].get("cpuMillis").is_some());
        assert!(encoded["template"]["artifact"].get("mediaType").is_some());
        for private in [
            "targetNodeId",
            "taskPolicy",
            "nodeId",
            "commandId",
            "cleanupCommandId",
            "runtimeSpecDigest",
        ] {
            assert!(encoded.get(private).is_none());
        }
    }
}
