use super::execution_task_policy::ExecutionTaskPolicy;
use super::execution_template::{valid_sha256, ExecutionTemplate};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, ExecutionId, ExecutionTemplateId,
    ExecutionTemplateRevisionId, NodeCommandId, NodeId, OperationId, OrganizationId,
    PlanRevisionId, ProjectId, Sha256Digest, WorkflowRunId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStatus {
    Queued,
    Scheduled,
    Running,
    Cancelling,
    CleanupPending,
    Succeeded,
    Failed,
    Cancelled,
}

impl ExecutionStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Scheduled => "scheduled",
            Self::Running => "running",
            Self::Cancelling => "cancelling",
            Self::CleanupPending => "cleanup_pending",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "queued" => Ok(Self::Queued),
            "scheduled" => Ok(Self::Scheduled),
            "running" => Ok(Self::Running),
            "cancelling" => Ok(Self::Cancelling),
            "cleanup_pending" => Ok(Self::CleanupPending),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(format!("unsupported execution status {value:?}")),
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExecutionOutcome {
    Succeeded {
        exit_code: i32,
    },
    Failed {
        exit_code: Option<i32>,
        reason: String,
    },
    Cancelled,
}

impl ExecutionOutcome {
    fn validate(&self) -> Result<(), String> {
        match self {
            Self::Succeeded { exit_code: 0 } | Self::Cancelled => Ok(()),
            Self::Succeeded { .. } => Err("successful execution must have exit code zero".into()),
            Self::Failed { reason, .. }
                if reason.is_empty()
                    || reason.len() > 16 * 1024
                    || reason.contains(['\0', '\r', '\n']) =>
            {
                Err("execution failure reason is invalid".into())
            }
            Self::Failed { .. } => Ok(()),
        }
    }

    const fn status(&self) -> ExecutionStatus {
        match self {
            Self::Succeeded { .. } => ExecutionStatus::Succeeded,
            Self::Failed { .. } => ExecutionStatus::Failed,
            Self::Cancelled => ExecutionStatus::Cancelled,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowExecutionBinding {
    pub workflow_run_id: WorkflowRunId,
    pub plan_revision_id: PlanRevisionId,
    pub plan_digest: Sha256Digest,
    pub step_id: String,
    pub step_attempt: u64,
    pub execution_template_id: ExecutionTemplateId,
    pub execution_template_revision_id: ExecutionTemplateRevisionId,
    pub execution_template_digest: Sha256Digest,
}

impl WorkflowExecutionBinding {
    pub fn validate(&self) -> Result<(), String> {
        if self.workflow_run_id.as_uuid().is_nil()
            || self.plan_revision_id.as_uuid().is_nil()
            || self.execution_template_id.as_uuid().is_nil()
            || self.execution_template_revision_id.as_uuid().is_nil()
            || self.step_attempt == 0
            || self.step_id.is_empty()
            || self.step_id.len() > 96
            || !self
                .step_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err("Workflow execution binding is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Execution {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub id: ExecutionId,
    pub operation_id: OperationId,
    pub workflow: Option<WorkflowExecutionBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_node_id: Option<NodeId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_policy: Option<ExecutionTaskPolicy>,
    pub template: ExecutionTemplate,
    pub template_digest: String,
    pub status: ExecutionStatus,
    pub node_id: Option<NodeId>,
    pub command_id: Option<NodeCommandId>,
    pub cleanup_command_id: Option<NodeCommandId>,
    pub runtime_spec_digest: Option<String>,
    pub outcome: Option<ExecutionOutcome>,
    pub aggregate_version: u64,
    pub requested_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub cancellation_requested_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
}

impl Execution {
    pub const RUNTIME_GENERATION: u64 = 1;

    pub fn create(
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        id: ExecutionId,
        template: ExecutionTemplate,
        requested_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        Self::create_with_workflow(
            organization_id,
            project_id,
            environment_id,
            id,
            template,
            None,
            requested_at,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_with_workflow(
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        id: ExecutionId,
        template: ExecutionTemplate,
        workflow: Option<WorkflowExecutionBinding>,
        requested_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        Self::create_with_bindings(
            organization_id,
            project_id,
            environment_id,
            id,
            template,
            workflow,
            None,
            None,
            requested_at,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_bound_task(
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        id: ExecutionId,
        template: ExecutionTemplate,
        target_node_id: NodeId,
        task_policy: ExecutionTaskPolicy,
        requested_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        Self::create_with_bindings(
            organization_id,
            project_id,
            environment_id,
            id,
            template,
            None,
            Some(target_node_id),
            Some(task_policy),
            requested_at,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn create_with_bindings(
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        id: ExecutionId,
        template: ExecutionTemplate,
        workflow: Option<WorkflowExecutionBinding>,
        target_node_id: Option<NodeId>,
        task_policy: Option<ExecutionTaskPolicy>,
        requested_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        if organization_id.as_uuid().is_nil()
            || project_id.as_uuid().is_nil()
            || environment_id.as_uuid().is_nil()
            || id.as_uuid().is_nil()
        {
            return Err("execution identity must not contain nil UUIDs".into());
        }
        let template_digest = template.digest()?;
        let requested_at = canonical_timestamp(requested_at);
        let execution = Self {
            organization_id,
            project_id,
            environment_id,
            id,
            operation_id: OperationId::from_uuid(id.as_uuid()),
            workflow,
            target_node_id,
            task_policy,
            template,
            template_digest,
            status: ExecutionStatus::Queued,
            node_id: None,
            command_id: None,
            cleanup_command_id: None,
            runtime_spec_digest: None,
            outcome: None,
            aggregate_version: 1,
            requested_at,
            updated_at: requested_at,
            started_at: None,
            cancellation_requested_at: None,
            finished_at: None,
        };
        execution.validate()?;
        Ok(execution)
    }

    pub fn restore(mut self) -> Result<Self, String> {
        self.requested_at = canonical_timestamp(self.requested_at);
        self.updated_at = canonical_timestamp(self.updated_at);
        self.started_at = self.started_at.map(canonical_timestamp);
        self.cancellation_requested_at = self.cancellation_requested_at.map(canonical_timestamp);
        self.finished_at = self.finished_at.map(canonical_timestamp);
        self.validate()?;
        Ok(self)
    }

    pub fn runtime_unit_id(&self) -> String {
        format!("cloud-execution-{}", self.id)
    }

    pub const fn is_bound_task(&self) -> bool {
        self.task_policy.is_some()
    }

    pub fn schedule(
        &mut self,
        node_id: NodeId,
        runtime_spec_digest: String,
        at: DateTime<Utc>,
    ) -> Result<(), String> {
        if self.status == ExecutionStatus::Scheduled {
            return if self.node_id == Some(node_id)
                && self.runtime_spec_digest.as_ref() == Some(&runtime_spec_digest)
            {
                self.observe_time(at)
            } else {
                Err("scheduled execution cannot change node or Runtime specification".into())
            };
        }
        if self.status != ExecutionStatus::Queued
            || node_id.as_uuid().is_nil()
            || !valid_sha256(&runtime_spec_digest)
            || self
                .target_node_id
                .is_some_and(|target_node_id| target_node_id != node_id)
        {
            return Err("execution cannot be scheduled with the supplied Runtime identity".into());
        }
        self.transition(ExecutionStatus::Scheduled, at)?;
        self.node_id = Some(node_id);
        self.runtime_spec_digest = Some(runtime_spec_digest);
        Ok(())
    }

    pub fn dispatch(&mut self, command_id: NodeCommandId, at: DateTime<Utc>) -> Result<(), String> {
        if self.status == ExecutionStatus::Running {
            return if self.command_id == Some(command_id) {
                self.observe_time(at)
            } else {
                Err("running execution cannot change its Runtime command".into())
            };
        }
        if self.status != ExecutionStatus::Scheduled || command_id.as_uuid().is_nil() {
            return Err("execution cannot dispatch from its current state".into());
        }
        self.transition(ExecutionStatus::Running, at)?;
        self.command_id = Some(command_id);
        self.started_at.get_or_insert(self.updated_at);
        Ok(())
    }

    pub fn request_cancellation(&mut self, at: DateTime<Utc>) -> Result<(), String> {
        if self.status.is_terminal() || self.status == ExecutionStatus::CleanupPending {
            return Err("terminal or cleaning execution cannot be cancelled".into());
        }
        if self.status != ExecutionStatus::Cancelling {
            self.transition(ExecutionStatus::Cancelling, at)?;
            self.cancellation_requested_at = Some(self.updated_at);
        } else {
            self.observe_time(at)?;
        }
        Ok(())
    }

    pub fn begin_cleanup(
        &mut self,
        outcome: ExecutionOutcome,
        at: DateTime<Utc>,
    ) -> Result<(), String> {
        outcome.validate()?;
        if self.status == ExecutionStatus::CleanupPending {
            return if self.outcome.as_ref() == Some(&outcome) {
                self.observe_time(at)
            } else {
                Err("cleaning execution cannot change its outcome".into())
            };
        }
        if !matches!(
            self.status,
            ExecutionStatus::Queued
                | ExecutionStatus::Scheduled
                | ExecutionStatus::Running
                | ExecutionStatus::Cancelling
        ) {
            return Err("execution cannot begin cleanup from its current state".into());
        }
        self.transition(ExecutionStatus::CleanupPending, at)?;
        self.outcome = Some(outcome);
        Ok(())
    }

    pub fn record_cleanup_command(
        &mut self,
        command_id: NodeCommandId,
        at: DateTime<Utc>,
    ) -> Result<(), String> {
        if self.status != ExecutionStatus::CleanupPending || command_id.as_uuid().is_nil() {
            return Err("execution cleanup command is invalid for the current state".into());
        }
        if self.cleanup_command_id == Some(command_id) {
            return self.observe_time(at);
        }
        self.cleanup_command_id = Some(command_id);
        self.bump(at)
    }

    pub fn complete_cleanup(&mut self, at: DateTime<Utc>) -> Result<(), String> {
        if self.status.is_terminal() {
            return self.observe_time(at);
        }
        if self.status != ExecutionStatus::CleanupPending {
            return Err("execution cannot complete before cleanup is pending".into());
        }
        let outcome = self
            .outcome
            .as_ref()
            .ok_or_else(|| "execution cleanup has no terminal outcome".to_owned())?;
        self.transition(outcome.status(), at)?;
        self.finished_at = Some(self.updated_at);
        Ok(())
    }

    pub fn validate(&self) -> Result<(), String> {
        self.template.validate()?;
        if let Some(workflow) = &self.workflow {
            workflow.validate()?;
        }
        match (self.target_node_id, self.task_policy.as_ref()) {
            (None, None) => {}
            (Some(target_node_id), Some(task_policy)) if self.workflow.is_none() => {
                task_policy.validate(target_node_id, &self.template)?;
            }
            _ => return Err(
                "execution bound Task policy, target node, and Workflow authority are inconsistent"
                    .into(),
            ),
        }
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.id.as_uuid().is_nil()
            || self.operation_id.as_uuid() != self.id.as_uuid()
            || self.template.digest()? != self.template_digest
            || self.aggregate_version == 0
            || self.updated_at < self.requested_at
            || self
                .started_at
                .is_some_and(|value| value < self.requested_at)
            || self
                .cancellation_requested_at
                .is_some_and(|value| value < self.requested_at)
            || self
                .finished_at
                .is_some_and(|value| value < self.requested_at)
            || self
                .runtime_spec_digest
                .as_ref()
                .is_some_and(|digest| !valid_sha256(digest))
            || self
                .node_id
                .zip(self.target_node_id)
                .is_some_and(|(node_id, target_node_id)| node_id != target_node_id)
        {
            return Err("execution aggregate is invalid".into());
        }
        if self.status == ExecutionStatus::Queued
            && (self.node_id.is_some()
                || self.command_id.is_some()
                || self.runtime_spec_digest.is_some())
        {
            return Err("queued execution contains Runtime scheduling state".into());
        }
        if matches!(
            self.status,
            ExecutionStatus::Scheduled | ExecutionStatus::Running
        ) && (self.node_id.is_none() || self.runtime_spec_digest.is_none())
        {
            return Err("scheduled execution is missing its Runtime identity".into());
        }
        if self.status == ExecutionStatus::Running && self.command_id.is_none() {
            return Err("running execution is missing its Runtime command".into());
        }
        if matches!(
            self.status,
            ExecutionStatus::CleanupPending
                | ExecutionStatus::Succeeded
                | ExecutionStatus::Failed
                | ExecutionStatus::Cancelled
        ) {
            self.outcome
                .as_ref()
                .ok_or_else(|| "terminal execution is missing its outcome".to_owned())?
                .validate()?;
        } else if self.outcome.is_some() {
            return Err("non-terminal execution contains a terminal outcome".into());
        }
        if self.status.is_terminal() != self.finished_at.is_some() {
            return Err("execution terminal timestamp does not match its status".into());
        }
        Ok(())
    }

    fn transition(&mut self, status: ExecutionStatus, at: DateTime<Utc>) -> Result<(), String> {
        self.observe_time(at)?;
        self.status = status;
        self.aggregate_version = self
            .aggregate_version
            .checked_add(1)
            .ok_or_else(|| "execution aggregate version overflowed".to_owned())?;
        Ok(())
    }

    fn observe_time(&mut self, at: DateTime<Utc>) -> Result<(), String> {
        let at = canonical_timestamp(at);
        if at < self.updated_at {
            return Err("execution transition time regressed".into());
        }
        self.updated_at = at;
        Ok(())
    }

    fn bump(&mut self, at: DateTime<Utc>) -> Result<(), String> {
        self.observe_time(at)?;
        self.aggregate_version = self
            .aggregate_version
            .checked_add(1)
            .ok_or_else(|| "execution aggregate version overflowed".to_owned())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::executions::domain::{
        ExecutionArtifact, ExecutionProcess, ExecutionResources,
    };
    use std::collections::BTreeMap;

    fn template() -> ExecutionTemplate {
        let digest = format!("sha256:{}", "a".repeat(64));
        ExecutionTemplate {
            artifact: ExecutionArtifact {
                uri: format!("oci://registry.example/a3s/function@{digest}"),
                digest,
                media_type: "application/vnd.oci.image.manifest.v1+json".into(),
            },
            process: ExecutionProcess {
                command: vec!["/usr/bin/function".into()],
                args: vec!["invoke".into()],
                working_directory: Some("/workspace".into()),
                environment: BTreeMap::from([("MODE".into(), "batch".into())]),
            },
            input: serde_json::json!({"message": "hello"}),
            resources: ExecutionResources {
                cpu_millis: 500,
                memory_bytes: 256 * 1024 * 1024,
                pids: 128,
                ephemeral_storage_bytes: None,
                timeout_ms: 30_000,
            },
        }
    }

    #[test]
    fn execution_lifecycle_is_generation_bound_and_cleanup_first() {
        let requested_at = Utc::now();
        let mut execution = Execution::create(
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            ExecutionId::new(),
            template(),
            requested_at,
        )
        .expect("execution");
        let node_id = NodeId::new();
        let spec_digest = format!("sha256:{}", "b".repeat(64));
        execution
            .schedule(node_id, spec_digest, requested_at)
            .expect("schedule");
        execution
            .dispatch(NodeCommandId::new(), requested_at)
            .expect("dispatch");
        execution
            .begin_cleanup(ExecutionOutcome::Succeeded { exit_code: 0 }, requested_at)
            .expect("cleanup");
        assert_eq!(execution.status, ExecutionStatus::CleanupPending);
        assert!(execution.finished_at.is_none());
        execution.complete_cleanup(requested_at).expect("complete");
        assert_eq!(execution.status, ExecutionStatus::Succeeded);
        execution.validate().expect("valid execution");
    }

    #[test]
    fn execution_template_rejects_mutable_images_reserved_environment_and_large_input() {
        let mut mutable = template();
        mutable.artifact.uri = "oci://registry.example/a3s/function:latest".into();
        assert!(mutable.validate().is_err());

        let mut reserved = template();
        reserved
            .process
            .environment
            .insert("A3S_EXECUTION_INPUT".into(), "forged".into());
        assert!(reserved.validate().is_err());

        let mut oversized = template();
        oversized.input = serde_json::Value::String("x".repeat(16 * 1024 + 1));
        assert!(oversized.validate().is_err());
    }

    #[test]
    fn cancellation_requires_cleanup_before_terminal_state() {
        let now = Utc::now();
        let mut execution = Execution::create(
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            ExecutionId::new(),
            template(),
            now,
        )
        .expect("execution");
        execution
            .request_cancellation(now)
            .expect("request cancellation");
        assert_eq!(execution.status, ExecutionStatus::Cancelling);
        execution
            .begin_cleanup(ExecutionOutcome::Cancelled, now)
            .expect("begin cleanup");
        execution.complete_cleanup(now).expect("complete cleanup");
        assert_eq!(execution.status, ExecutionStatus::Cancelled);
    }
}
