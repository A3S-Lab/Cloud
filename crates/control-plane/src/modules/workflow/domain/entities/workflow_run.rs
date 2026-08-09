use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, canonical_timestamp, sha256_digest, OperationId, OrganizationId,
    PlanRevisionId, PrincipalId, ProjectId, Sha256Digest, WorkflowGoalId, WorkflowRunId,
};
use crate::modules::workflow::domain::{
    WorkflowRunInput, WorkflowStepProjection, WORKFLOW_RUN_OUTPUT_MAX_BYTES,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowRunStatus {
    Pending,
    Running,
    Waiting,
    Cancelling,
    Completed,
    Failed,
    Cancelled,
    TimedOut,
}

impl WorkflowRunStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Waiting => "waiting",
            Self::Cancelling => "cancelling",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::TimedOut => "timed_out",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
            "waiting" => Ok(Self::Waiting),
            "cancelling" => Ok(Self::Cancelling),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            "timed_out" => Ok(Self::TimedOut),
            _ => Err(format!("unsupported WorkflowRun status {value:?}")),
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Cancelled | Self::TimedOut
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowRun {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub id: WorkflowRunId,
    pub workflow_goal_id: WorkflowGoalId,
    pub plan_revision_id: PlanRevisionId,
    pub plan_digest: Sha256Digest,
    pub operation_id: OperationId,
    pub flow_run_id: String,
    pub flow_runtime_build_id: Option<String>,
    pub execution_input: WorkflowRunInput,
    pub execution_input_digest: Sha256Digest,
    pub status: WorkflowRunStatus,
    pub last_flow_sequence: u64,
    pub output: Option<serde_json::Value>,
    pub output_digest: Option<Sha256Digest>,
    pub error: Option<String>,
    pub aggregate_version: u64,
    pub requested_by: PrincipalId,
    pub requested_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub cancellation_requested_at: Option<DateTime<Utc>>,
    pub cancellation_reason: Option<String>,
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowRunFlowState {
    pub status: WorkflowRunStatus,
    pub flow_runtime_build_id: String,
    pub last_flow_sequence: u64,
    pub output: Option<serde_json::Value>,
    pub error: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub observed_at: DateTime<Utc>,
}

impl WorkflowRun {
    pub fn create(
        input: WorkflowRunInput,
        requested_by: PrincipalId,
    ) -> Result<(Self, Vec<WorkflowStepProjection>), String> {
        input.validate()?;
        let canonical = input.canonical_bytes()?;
        let execution_input_digest = Sha256Digest::parse(sha256_digest(&canonical))?;
        let requested_at = canonical_timestamp(input.requested_at);
        let value = Self {
            organization_id: input.organization_id,
            project_id: input.project_id,
            id: input.workflow_run_id,
            workflow_goal_id: input.workflow_goal_id,
            plan_revision_id: input.plan_revision_id,
            plan_digest: input.plan_digest.clone(),
            operation_id: OperationId::from_uuid(input.workflow_run_id.as_uuid()),
            flow_run_id: input.workflow_run_id.to_string(),
            flow_runtime_build_id: None,
            execution_input: input,
            execution_input_digest,
            status: WorkflowRunStatus::Pending,
            last_flow_sequence: 0,
            output: None,
            output_digest: None,
            error: None,
            aggregate_version: 1,
            requested_by,
            requested_at,
            updated_at: requested_at,
            started_at: None,
            cancellation_requested_at: None,
            cancellation_reason: None,
            finished_at: None,
        };
        let steps = value
            .execution_input
            .plan
            .steps
            .iter()
            .map(|step| {
                WorkflowStepProjection::pending(
                    value.organization_id,
                    value.project_id,
                    value.id,
                    step.id.clone(),
                    step.kind,
                    requested_at,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        value.validate()?;
        Ok((value, steps))
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

    pub fn request_cancellation(
        &mut self,
        reason: Option<String>,
        requested_at: DateTime<Utc>,
    ) -> Result<(), String> {
        if self.status.is_terminal() {
            return Err("terminal WorkflowRun cannot be cancelled".into());
        }
        if self.status == WorkflowRunStatus::Cancelling {
            return if self.cancellation_reason == reason {
                Err("WorkflowRun cancellation was already requested".into())
            } else {
                Err("WorkflowRun cancellation reason cannot drift".into())
            };
        }
        validate_optional_reason(reason.as_deref())?;
        let requested_at = canonical_timestamp(requested_at);
        if requested_at < self.updated_at {
            return Err("WorkflowRun cancellation time precedes its current state".into());
        }
        self.status = WorkflowRunStatus::Cancelling;
        self.cancellation_requested_at = Some(requested_at);
        self.cancellation_reason = reason;
        self.updated_at = requested_at;
        self.aggregate_version = self
            .aggregate_version
            .checked_add(1)
            .ok_or_else(|| "WorkflowRun aggregate version overflowed".to_owned())?;
        self.validate()
    }

    pub fn project_flow(&mut self, state: WorkflowRunFlowState) -> Result<bool, String> {
        if state.last_flow_sequence < self.last_flow_sequence {
            return Ok(false);
        }
        if state.flow_runtime_build_id.is_empty()
            || state.flow_runtime_build_id.len() > 255
            || state.flow_runtime_build_id.contains('\0')
        {
            return Err("WorkflowRun Flow runtime build identity is invalid".into());
        }
        if self
            .flow_runtime_build_id
            .as_ref()
            .is_some_and(|existing| existing != &state.flow_runtime_build_id)
        {
            return Err("WorkflowRun Flow runtime build identity drifted".into());
        }
        let projected_status = if self.status == WorkflowRunStatus::Cancelling
            && matches!(
                state.status,
                WorkflowRunStatus::Pending
                    | WorkflowRunStatus::Running
                    | WorkflowRunStatus::Waiting
            ) {
            WorkflowRunStatus::Cancelling
        } else {
            state.status
        };
        let output_digest = state
            .output
            .as_ref()
            .map(|output| {
                canonical_json_bounded(output, WORKFLOW_RUN_OUTPUT_MAX_BYTES, "WorkflowRun output")
                    .and_then(|canonical| Sha256Digest::parse(sha256_digest(&canonical)))
            })
            .transpose()?;
        let observed_at = canonical_timestamp(state.observed_at);
        let started_at = state.started_at.map(canonical_timestamp);
        let finished_at = state.finished_at.map(canonical_timestamp);
        if state.last_flow_sequence == self.last_flow_sequence {
            let identical = self.status == projected_status
                && self.flow_runtime_build_id.as_ref() == Some(&state.flow_runtime_build_id)
                && self.output == state.output
                && self.output_digest == output_digest
                && self.error == state.error
                && self.started_at == started_at
                && self.finished_at == finished_at;
            let cancellation_is_awaiting_flow = self.status == WorkflowRunStatus::Cancelling
                && projected_status == WorkflowRunStatus::Cancelling
                && self.flow_runtime_build_id.as_ref() == Some(&state.flow_runtime_build_id);
            return if identical || cancellation_is_awaiting_flow {
                Ok(false)
            } else {
                Err("WorkflowRun projection drifted without a new Flow sequence".into())
            };
        }
        if self.status.is_terminal() {
            return Err("terminal WorkflowRun projection cannot change".into());
        }
        if observed_at < self.updated_at {
            return Err("WorkflowRun projection time moved backwards".into());
        }
        self.status = projected_status;
        self.flow_runtime_build_id = Some(state.flow_runtime_build_id);
        self.last_flow_sequence = state.last_flow_sequence;
        self.output = state.output;
        self.output_digest = output_digest;
        self.error = state.error;
        self.started_at = started_at.or(self.started_at);
        self.finished_at = finished_at;
        self.updated_at = observed_at;
        self.aggregate_version = self
            .aggregate_version
            .checked_add(1)
            .ok_or_else(|| "WorkflowRun aggregate version overflowed".to_owned())?;
        self.validate()?;
        Ok(true)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.execution_input.validate()?;
        let canonical = self.execution_input.canonical_bytes()?;
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.id.as_uuid().is_nil()
            || self.workflow_goal_id.as_uuid().is_nil()
            || self.plan_revision_id.as_uuid().is_nil()
            || self.requested_by.as_uuid().is_nil()
            || self.operation_id.as_uuid() != self.id.as_uuid()
            || self.flow_run_id != self.id.to_string()
            || self.execution_input.organization_id != self.organization_id
            || self.execution_input.project_id != self.project_id
            || self.execution_input.workflow_run_id != self.id
            || self.execution_input.workflow_goal_id != self.workflow_goal_id
            || self.execution_input.plan_revision_id != self.plan_revision_id
            || self.execution_input.plan_digest != self.plan_digest
            || sha256_digest(&canonical) != self.execution_input_digest.as_str()
            || self.aggregate_version == 0
            || self.updated_at < self.requested_at
            || self
                .flow_runtime_build_id
                .as_deref()
                .is_some_and(|value| value.is_empty() || value.len() > 255 || value.contains('\0'))
        {
            return Err("WorkflowRun identity, immutable input, or version is invalid".into());
        }
        match (&self.output, &self.output_digest) {
            (Some(output), Some(digest)) => {
                let canonical = canonical_json_bounded(
                    output,
                    WORKFLOW_RUN_OUTPUT_MAX_BYTES,
                    "WorkflowRun output",
                )?;
                if sha256_digest(&canonical) != digest.as_str() {
                    return Err("WorkflowRun output digest does not match its value".into());
                }
            }
            (None, None) => {}
            _ => return Err("WorkflowRun output and digest must be stored together".into()),
        }
        validate_optional_reason(self.cancellation_reason.as_deref())?;
        if self.error.as_deref().is_some_and(|value| {
            value.is_empty() || value.len() > 16 * 1024 || value.contains('\0')
        }) {
            return Err("WorkflowRun error is invalid".into());
        }
        if self.status == WorkflowRunStatus::Completed && self.output.is_none() {
            return Err("completed WorkflowRun is missing its output".into());
        }
        if self.status != WorkflowRunStatus::Completed && self.output.is_some() {
            return Err("non-completed WorkflowRun contains terminal output".into());
        }
        if matches!(
            self.status,
            WorkflowRunStatus::Failed | WorkflowRunStatus::TimedOut
        ) && self.error.is_none()
        {
            return Err("failed WorkflowRun is missing its error".into());
        }
        if !matches!(
            self.status,
            WorkflowRunStatus::Failed | WorkflowRunStatus::TimedOut
        ) && self.error.is_some()
        {
            return Err("non-failed WorkflowRun contains an error".into());
        }
        if self.status == WorkflowRunStatus::Cancelling && self.cancellation_requested_at.is_none()
        {
            return Err("cancelling WorkflowRun has no cancellation request".into());
        }
        if self.cancellation_reason.is_some() && self.cancellation_requested_at.is_none() {
            return Err("WorkflowRun cancellation reason has no request time".into());
        }
        if self.status.is_terminal() != self.finished_at.is_some() {
            return Err("WorkflowRun terminal timestamp does not match its status".into());
        }
        for timestamp in [
            self.started_at,
            self.cancellation_requested_at,
            self.finished_at,
        ]
        .into_iter()
        .flatten()
        {
            if timestamp < self.requested_at {
                return Err("WorkflowRun lifecycle time precedes its request".into());
            }
        }
        Ok(())
    }
}

fn validate_optional_reason(reason: Option<&str>) -> Result<(), String> {
    if reason.is_some_and(|value| {
        value.is_empty() || value.len() > 4_096 || value.contains(['\0', '\r', '\n'])
    }) {
        Err("WorkflowRun cancellation reason is invalid".into())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::workflow::test_support::{timestamp, workflow_run_input};
    use serde_json::json;

    #[test]
    fn workflow_run_projects_monotonic_flow_state_and_preserves_cancellation() {
        let input = workflow_run_input().expect("WorkflowRun input");
        let (mut run, steps) = WorkflowRun::create(input, PrincipalId::new()).expect("WorkflowRun");
        assert_eq!(run.status, WorkflowRunStatus::Pending);
        assert_eq!(run.aggregate_version, 1);
        assert_eq!(steps.len(), 6);

        let running = WorkflowRunFlowState {
            status: WorkflowRunStatus::Running,
            flow_runtime_build_id: "cloud-flow-test-build".into(),
            last_flow_sequence: 1,
            output: None,
            error: None,
            started_at: Some(timestamp(8, 1)),
            finished_at: None,
            observed_at: timestamp(8, 1),
        };
        assert!(run
            .project_flow(running.clone())
            .expect("running projection"));
        assert!(!run.project_flow(running.clone()).expect("exact replay"));

        let mut drift = running;
        drift.status = WorkflowRunStatus::Waiting;
        assert!(run.project_flow(drift).is_err());

        run.request_cancellation(Some("operator request".into()), timestamp(8, 2))
            .expect("cancellation request");
        assert_eq!(run.status, WorkflowRunStatus::Cancelling);
        assert_eq!(run.aggregate_version, 3);
        assert!(run
            .project_flow(WorkflowRunFlowState {
                status: WorkflowRunStatus::Running,
                flow_runtime_build_id: "cloud-flow-test-build".into(),
                last_flow_sequence: 2,
                output: None,
                error: None,
                started_at: Some(timestamp(8, 1)),
                finished_at: None,
                observed_at: timestamp(8, 3),
            })
            .expect("cancelling projection"));
        assert_eq!(run.status, WorkflowRunStatus::Cancelling);
        assert!(run
            .project_flow(WorkflowRunFlowState {
                status: WorkflowRunStatus::Cancelled,
                flow_runtime_build_id: "cloud-flow-test-build".into(),
                last_flow_sequence: 3,
                output: None,
                error: None,
                started_at: Some(timestamp(8, 1)),
                finished_at: Some(timestamp(8, 4)),
                observed_at: timestamp(8, 4),
            })
            .expect("cancelled projection"));
        assert_eq!(run.status, WorkflowRunStatus::Cancelled);
        assert!(run.request_cancellation(None, timestamp(8, 5)).is_err());
    }

    #[test]
    fn workflow_run_validates_completed_and_timed_out_terminal_state() {
        let input = workflow_run_input().expect("WorkflowRun input");
        let (mut completed, _) =
            WorkflowRun::create(input.clone(), PrincipalId::new()).expect("WorkflowRun");
        completed
            .project_flow(WorkflowRunFlowState {
                status: WorkflowRunStatus::Completed,
                flow_runtime_build_id: "cloud-flow-test-build".into(),
                last_flow_sequence: 8,
                output: Some(json!({"result": "HIGH T-42"})),
                error: None,
                started_at: Some(timestamp(8, 1)),
                finished_at: Some(timestamp(8, 4)),
                observed_at: timestamp(8, 4),
            })
            .expect("completed projection");
        assert_eq!(completed.status, WorkflowRunStatus::Completed);
        assert!(completed.output_digest.is_some());
        completed.validate().expect("valid completion");

        let (mut timed_out, _) =
            WorkflowRun::create(input, PrincipalId::new()).expect("WorkflowRun");
        timed_out
            .project_flow(WorkflowRunFlowState {
                status: WorkflowRunStatus::TimedOut,
                flow_runtime_build_id: "cloud-flow-test-build".into(),
                last_flow_sequence: 9,
                output: None,
                error: Some("immutable deadline exceeded".into()),
                started_at: Some(timestamp(8, 1)),
                finished_at: Some(timestamp(9, 0)),
                observed_at: timestamp(9, 0),
            })
            .expect("timeout projection");
        assert_eq!(timed_out.status, WorkflowRunStatus::TimedOut);
        timed_out.validate().expect("valid timeout");
    }
}
