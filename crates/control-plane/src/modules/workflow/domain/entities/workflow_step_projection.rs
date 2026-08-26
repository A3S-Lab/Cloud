use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, canonical_timestamp, sha256_digest, OrganizationId, ProjectId,
    Sha256Digest, WorkflowRunId,
};
use crate::modules::workflow::domain::{
    validate_evidence_references, WorkflowStepDefaultOutputEvidence, WorkflowStepKind,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const WORKFLOW_STEP_RESULT_MAX_BYTES: usize = 256 * 1024;
pub const WORKFLOW_STEP_MAX_EVIDENCE_REFERENCES: usize = 32;
pub const WORKFLOW_STEP_EVIDENCE_REFERENCE_MAX_BYTES: usize = 1_024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowStepProjectionStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
    Skipped,
}

impl WorkflowStepProjectionStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Skipped => "skipped",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            "skipped" => Ok(Self::Skipped),
            _ => Err(format!(
                "unsupported Workflow step projection status {value:?}"
            )),
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Cancelled | Self::Skipped
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowStepProjection {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub workflow_run_id: WorkflowRunId,
    pub step_id: String,
    pub kind: WorkflowStepKind,
    pub status: WorkflowStepProjectionStatus,
    pub flow_step_id: String,
    pub attempt_generation: u32,
    pub selected_handle: Option<String>,
    pub result: Option<serde_json::Value>,
    pub result_digest: Option<Sha256Digest>,
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_output_evidence: Option<WorkflowStepDefaultOutputEvidence>,
    pub evidence_references: Vec<String>,
    pub last_flow_sequence: u64,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowStepFlowState {
    pub status: WorkflowStepProjectionStatus,
    pub attempt_generation: u32,
    pub selected_handle: Option<String>,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
    pub default_output_evidence: Option<WorkflowStepDefaultOutputEvidence>,
    pub evidence_references: Vec<String>,
    pub last_flow_sequence: u64,
    pub observed_at: DateTime<Utc>,
}

impl WorkflowStepProjection {
    pub fn pending(
        organization_id: OrganizationId,
        project_id: ProjectId,
        workflow_run_id: WorkflowRunId,
        step_id: String,
        kind: WorkflowStepKind,
        requested_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let value = Self {
            organization_id,
            project_id,
            workflow_run_id,
            flow_step_id: flow_step_id(&step_id),
            step_id,
            kind,
            status: WorkflowStepProjectionStatus::Pending,
            attempt_generation: 0,
            selected_handle: None,
            result: None,
            result_digest: None,
            error: None,
            default_output_evidence: None,
            evidence_references: Vec::new(),
            last_flow_sequence: 0,
            updated_at: canonical_timestamp(requested_at),
        };
        value.validate()?;
        Ok(value)
    }

    pub fn restore(mut self) -> Result<Self, String> {
        self.updated_at = canonical_timestamp(self.updated_at);
        self.validate()?;
        Ok(self)
    }

    pub fn project_flow(&mut self, state: WorkflowStepFlowState) -> Result<bool, String> {
        if state.last_flow_sequence < self.last_flow_sequence {
            return Ok(false);
        }
        let result_digest = state
            .result
            .as_ref()
            .map(|result| {
                canonical_json_bounded(
                    result,
                    WORKFLOW_STEP_RESULT_MAX_BYTES,
                    "Workflow step result",
                )
                .and_then(|canonical| Sha256Digest::parse(sha256_digest(&canonical)))
            })
            .transpose()?;
        let observed_at = canonical_timestamp(state.observed_at);
        if state.last_flow_sequence == self.last_flow_sequence {
            let identical = self.status == state.status
                && self.attempt_generation == state.attempt_generation
                && self.selected_handle == state.selected_handle
                && self.result == state.result
                && self.result_digest == result_digest
                && self.error == state.error
                && self.default_output_evidence == state.default_output_evidence
                && self.evidence_references == state.evidence_references;
            return if identical {
                Ok(false)
            } else {
                Err("Workflow step projection drifted without a new Flow sequence".into())
            };
        }
        if self.status.is_terminal() {
            return Err("terminal Workflow step projection cannot change".into());
        }
        if observed_at < self.updated_at {
            return Err("Workflow step projection time moved backwards".into());
        }
        self.status = state.status;
        self.attempt_generation = state.attempt_generation;
        self.selected_handle = state.selected_handle;
        self.result = state.result;
        self.result_digest = result_digest;
        self.error = state.error;
        self.default_output_evidence = state.default_output_evidence;
        self.evidence_references = state.evidence_references;
        self.last_flow_sequence = state.last_flow_sequence;
        self.updated_at = observed_at;
        self.validate()?;
        Ok(true)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.workflow_run_id.as_uuid().is_nil()
            || !valid_identifier(&self.step_id)
            || self.flow_step_id != flow_step_id(&self.step_id)
            || self
                .selected_handle
                .as_deref()
                .is_some_and(|value| !valid_identifier(value))
            || self
                .error
                .as_deref()
                .is_some_and(|value| !valid_error(value))
        {
            return Err("Workflow step projection identity or bounded state is invalid".into());
        }
        validate_evidence_references(&self.evidence_references)?;
        match (&self.result, &self.result_digest) {
            (Some(result), Some(digest)) => {
                let canonical = canonical_json_bounded(
                    result,
                    WORKFLOW_STEP_RESULT_MAX_BYTES,
                    "Workflow step result",
                )?;
                if sha256_digest(&canonical) != digest.as_str() {
                    return Err("Workflow step result digest does not match its value".into());
                }
            }
            (None, None) => {}
            _ => return Err("Workflow step result and digest must be stored together".into()),
        }
        if self.status == WorkflowStepProjectionStatus::Completed && self.result.is_none() {
            return Err("completed Workflow step projection is missing its result".into());
        }
        if self.status != WorkflowStepProjectionStatus::Completed && self.result.is_some() {
            return Err("non-completed Workflow step projection contains a result".into());
        }
        if self.status == WorkflowStepProjectionStatus::Failed && self.error.is_none() {
            return Err("failed Workflow step projection is missing its error".into());
        }
        if self.status != WorkflowStepProjectionStatus::Failed && self.error.is_some() {
            return Err("non-failed Workflow step projection contains an error".into());
        }
        if let Some(evidence) = self.default_output_evidence.as_ref() {
            if self.kind != WorkflowStepKind::Execution
                || self.status != WorkflowStepProjectionStatus::Completed
                || self.result.is_none()
                || self.selected_handle.is_some()
                || self.error.is_some()
            {
                return Err("Workflow default-output projection state is invalid".into());
            }
            evidence.validate_projection_shape(&self.step_id)?;
        }
        if self.selected_handle.is_some()
            && self.kind != WorkflowStepKind::Branch
            && !(matches!(
                self.kind,
                WorkflowStepKind::Transform
                    | WorkflowStepKind::Execution
                    | WorkflowStepKind::Agent
                    | WorkflowStepKind::Service
                    | WorkflowStepKind::Output
                    | WorkflowStepKind::Subworkflow
            ) && self.status == WorkflowStepProjectionStatus::Failed)
        {
            return Err(
                "only a Workflow branch or routed descriptor failure may select a handle".into(),
            );
        }
        Ok(())
    }
}

pub fn flow_step_id(step_id: &str) -> String {
    format!("workflow:{step_id}")
}

fn valid_identifier(value: &str) -> bool {
    let mut bytes = value.bytes();
    value.len() <= 128
        && bytes.next().is_some_and(|byte| byte.is_ascii_alphabetic())
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn valid_error(value: &str) -> bool {
    !value.is_empty() && value.len() <= 16 * 1024 && !value.contains('\0')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::workflow::domain::WorkflowStepFailureOutput;
    use crate::modules::workflow::test_support::{
        default_output_execution_workflow_run_input, timestamp, TEST_EXECUTION_STEP_ID,
    };
    use serde_json::json;

    #[test]
    fn step_projection_is_digest_bound_and_terminal() {
        let mut step = WorkflowStepProjection::pending(
            OrganizationId::new(),
            ProjectId::new(),
            WorkflowRunId::new(),
            "route".into(),
            WorkflowStepKind::Branch,
            timestamp(8, 0),
        )
        .expect("pending projection");
        let state = WorkflowStepFlowState {
            status: WorkflowStepProjectionStatus::Completed,
            attempt_generation: 1,
            selected_handle: Some("high".into()),
            result: Some(json!({"priority": "high"})),
            error: None,
            default_output_evidence: None,
            evidence_references: Vec::new(),
            last_flow_sequence: 4,
            observed_at: timestamp(8, 2),
        };
        assert!(step.project_flow(state.clone()).expect("completion"));
        assert!(step.result_digest.is_some());
        assert!(!step.project_flow(state).expect("exact replay"));
        assert!(step
            .project_flow(WorkflowStepFlowState {
                status: WorkflowStepProjectionStatus::Completed,
                attempt_generation: 2,
                selected_handle: Some("high".into()),
                result: Some(json!({"priority": "high"})),
                error: None,
                default_output_evidence: None,
                evidence_references: Vec::new(),
                last_flow_sequence: 5,
                observed_at: timestamp(8, 3),
            })
            .is_err());
    }

    #[test]
    fn completed_execution_projection_preserves_default_output_evidence() {
        let input = default_output_execution_workflow_run_input().expect("default-output input");
        let resolved = input.resolved_steps().expect("resolved steps");
        let step = resolved
            .iter()
            .find(|step| step.plan.id == TEST_EXECUTION_STEP_ID)
            .expect("Execution step");
        let failure = WorkflowStepFailureOutput::observe_dispatch_rejected(
            step,
            "provider unavailable".into(),
        )
        .expect("failure observation");
        let evidence = WorkflowStepDefaultOutputEvidence::new(step, failure).expect("evidence");
        let material = step
            .policy
            .as_ref()
            .and_then(|policy| policy.default_output.as_ref())
            .expect("default material");
        let mut projection = WorkflowStepProjection::pending(
            input.organization_id,
            input.project_id,
            input.workflow_run_id,
            TEST_EXECUTION_STEP_ID.into(),
            WorkflowStepKind::Execution,
            input.requested_at,
        )
        .expect("pending projection");
        projection
            .project_flow(WorkflowStepFlowState {
                status: WorkflowStepProjectionStatus::Completed,
                attempt_generation: 1,
                selected_handle: None,
                result: Some(material.value.clone()),
                error: None,
                default_output_evidence: Some(evidence.clone()),
                evidence_references: Vec::new(),
                last_flow_sequence: 4,
                observed_at: timestamp(8, 2),
            })
            .expect("fallback projection");
        assert_eq!(projection.default_output_evidence, Some(evidence));
        projection.validate().expect("valid stored projection");
    }

    #[test]
    fn failed_service_projection_may_retain_a_descriptor_bound_handle() {
        let mut projection = WorkflowStepProjection::pending(
            OrganizationId::new(),
            ProjectId::new(),
            WorkflowRunId::new(),
            "invoke".into(),
            WorkflowStepKind::Service,
            timestamp(8, 0),
        )
        .expect("pending projection");
        projection
            .project_flow(WorkflowStepFlowState {
                status: WorkflowStepProjectionStatus::Failed,
                attempt_generation: 1,
                selected_handle: Some("error".into()),
                result: None,
                error: Some("provider outcome is indeterminate".into()),
                default_output_evidence: None,
                evidence_references: vec![format!(
                    "urn:a3s:cloud:connectors:attempt:{}",
                    uuid::Uuid::now_v7()
                )],
                last_flow_sequence: 4,
                observed_at: timestamp(8, 2),
            })
            .expect("routed Service failure projection");
        projection.validate().expect("valid stored projection");
    }

    #[test]
    fn failed_output_projection_may_retain_a_descriptor_bound_handle() {
        let mut projection = WorkflowStepProjection::pending(
            OrganizationId::new(),
            ProjectId::new(),
            WorkflowRunId::new(),
            "answer".into(),
            WorkflowStepKind::Output,
            timestamp(8, 0),
        )
        .expect("pending projection");
        projection
            .project_flow(WorkflowStepFlowState {
                status: WorkflowStepProjectionStatus::Failed,
                attempt_generation: 1,
                selected_handle: Some("error".into()),
                result: None,
                error: Some("Application Answer was forbidden".into()),
                default_output_evidence: None,
                evidence_references: Vec::new(),
                last_flow_sequence: 4,
                observed_at: timestamp(8, 2),
            })
            .expect("routed Output failure projection");
        projection.validate().expect("valid stored projection");
    }

    #[test]
    fn failed_transform_projection_may_retain_a_descriptor_bound_handle() {
        let mut projection = WorkflowStepProjection::pending(
            OrganizationId::new(),
            ProjectId::new(),
            WorkflowRunId::new(),
            "transform".into(),
            WorkflowStepKind::Transform,
            timestamp(8, 0),
        )
        .expect("pending projection");
        projection
            .project_flow(WorkflowStepFlowState {
                status: WorkflowStepProjectionStatus::Failed,
                attempt_generation: 1,
                selected_handle: Some("error".into()),
                result: None,
                error: Some("Workflow Transform evaluation was invalid".into()),
                default_output_evidence: None,
                evidence_references: Vec::new(),
                last_flow_sequence: 4,
                observed_at: timestamp(8, 2),
            })
            .expect("routed Transform failure projection");
        projection.validate().expect("valid stored projection");
    }
}
