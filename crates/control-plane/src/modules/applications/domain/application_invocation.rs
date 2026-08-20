use super::{
    ApplicationRelease, ApplicationResponseMode, ApplicationSession, ApplicationSessionStatus,
};
use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, canonical_timestamp, ApplicationId, ApplicationInvocationId,
    ApplicationReleaseId, ApplicationSessionId, OrganizationId, ProjectId, Sha256Digest,
    WorkflowRunId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const APPLICATION_INVOCATION_INPUT_MAX_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationInvocationStatus {
    Requested,
    Running,
    Cancelling,
    Succeeded,
    Failed,
    Cancelled,
}

impl ApplicationInvocationStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Requested => "requested",
            Self::Running => "running",
            Self::Cancelling => "cancelling",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "requested" => Ok(Self::Requested),
            "running" => Ok(Self::Running),
            "cancelling" => Ok(Self::Cancelling),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(format!(
                "unsupported Application invocation status {value:?}"
            )),
        }
    }
}

/// Applications-owned invocation correlation state.
///
/// Workflow and Flow remain authoritative for graph compilation, execution,
/// attempts, cancellation, history, and output. This record only pins the
/// channel-visible Application request to that external run identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplicationInvocation {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub application_release_id: ApplicationReleaseId,
    pub application_release_digest: Sha256Digest,
    pub session_id: ApplicationSessionId,
    pub id: ApplicationInvocationId,
    pub response_mode: ApplicationResponseMode,
    pub input: Value,
    pub input_digest: Sha256Digest,
    pub workflow_run_id: Option<WorkflowRunId>,
    pub status: ApplicationInvocationStatus,
    pub aggregate_version: u64,
    pub requested_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

impl ApplicationInvocation {
    pub fn request(
        id: ApplicationInvocationId,
        session: &ApplicationSession,
        release: &ApplicationRelease,
        response_mode: ApplicationResponseMode,
        input: Value,
        requested_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        session.validate_release(release)?;
        if session.status != ApplicationSessionStatus::Active {
            return Err("closed Application session cannot start an invocation".into());
        }
        if !release
            .contract
            .spec()
            .delivery
            .response_modes
            .contains(&response_mode)
        {
            return Err("Application response mode is not admitted by the exact release".into());
        }
        let input_digest = application_invocation_input_digest(&input)?;
        let requested_at = canonical_timestamp(requested_at);
        if requested_at < session.created_at {
            return Err("Application invocation cannot predate its session".into());
        }
        let value = Self {
            organization_id: session.organization_id,
            project_id: session.project_id,
            application_id: session.application_id,
            application_release_id: session.application_release_id,
            application_release_digest: session.application_release_digest.clone(),
            session_id: session.id,
            id,
            response_mode,
            input,
            input_digest,
            workflow_run_id: None,
            status: ApplicationInvocationStatus::Requested,
            aggregate_version: 1,
            requested_at,
            updated_at: requested_at,
            completed_at: None,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn bind_workflow_run(
        &self,
        expected_version: u64,
        workflow_run_id: WorkflowRunId,
        bound_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        self.validate()?;
        if expected_version == 0
            || expected_version != self.aggregate_version
            || self.status != ApplicationInvocationStatus::Requested
            || workflow_run_id.as_uuid().is_nil()
        {
            return Err(
                "Application invocation cannot bind a stale or duplicate WorkflowRun".into(),
            );
        }
        self.transition(
            ApplicationInvocationStatus::Running,
            Some(workflow_run_id),
            bound_at,
        )
    }

    pub fn request_cancellation(
        &self,
        expected_version: u64,
        requested_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        self.validate()?;
        if expected_version == 0
            || expected_version != self.aggregate_version
            || !matches!(
                self.status,
                ApplicationInvocationStatus::Requested | ApplicationInvocationStatus::Running
            )
        {
            return Err("Application invocation cancellation is stale or terminal".into());
        }
        self.transition(
            ApplicationInvocationStatus::Cancelling,
            self.workflow_run_id,
            requested_at,
        )
    }

    pub fn observe_terminal(
        &self,
        expected_version: u64,
        status: ApplicationInvocationStatus,
        completed_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        self.validate()?;
        if expected_version == 0
            || expected_version != self.aggregate_version
            || !status.is_terminal()
            || self.status.is_terminal()
            || (matches!(
                status,
                ApplicationInvocationStatus::Succeeded | ApplicationInvocationStatus::Failed
            ) && self.workflow_run_id.is_none())
        {
            return Err("Application invocation terminal observation is invalid".into());
        }
        self.transition(status, self.workflow_run_id, completed_at)
    }

    pub fn restore(mut self) -> Result<Self, String> {
        self.requested_at = canonical_timestamp(self.requested_at);
        self.updated_at = canonical_timestamp(self.updated_at);
        self.completed_at = self.completed_at.map(canonical_timestamp);
        self.validate()?;
        Ok(self)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.application_id.as_uuid().is_nil()
            || self.application_release_id.as_uuid().is_nil()
            || self.session_id.as_uuid().is_nil()
            || self.id.as_uuid().is_nil()
            || self.aggregate_version == 0
            || self
                .workflow_run_id
                .is_some_and(|workflow_run_id| workflow_run_id.as_uuid().is_nil())
            || Sha256Digest::parse(self.application_release_digest.as_str())?
                != self.application_release_digest
            || application_invocation_input_digest(&self.input)? != self.input_digest
            || self.requested_at != canonical_timestamp(self.requested_at)
            || self.updated_at != canonical_timestamp(self.updated_at)
            || self.updated_at < self.requested_at
            || self
                .completed_at
                .is_some_and(|completed_at| completed_at != canonical_timestamp(completed_at))
            || self.status.is_terminal() != self.completed_at.is_some()
            || self
                .completed_at
                .is_some_and(|completed_at| completed_at != self.updated_at)
            || (self.status == ApplicationInvocationStatus::Requested
                && self.workflow_run_id.is_some())
            || (matches!(
                self.status,
                ApplicationInvocationStatus::Running
                    | ApplicationInvocationStatus::Succeeded
                    | ApplicationInvocationStatus::Failed
            ) && self.workflow_run_id.is_none())
        {
            return Err("stored Application invocation is invalid".into());
        }
        let valid_version = match (self.status, self.workflow_run_id.is_some()) {
            (ApplicationInvocationStatus::Requested, false) => self.aggregate_version == 1,
            (ApplicationInvocationStatus::Running, true) => self.aggregate_version == 2,
            (ApplicationInvocationStatus::Cancelling, false) => self.aggregate_version == 2,
            (ApplicationInvocationStatus::Cancelling, true) => self.aggregate_version == 3,
            (
                ApplicationInvocationStatus::Succeeded | ApplicationInvocationStatus::Failed,
                true,
            ) => matches!(self.aggregate_version, 3 | 4),
            (ApplicationInvocationStatus::Cancelled, false) => {
                matches!(self.aggregate_version, 2 | 3)
            }
            (ApplicationInvocationStatus::Cancelled, true) => {
                matches!(self.aggregate_version, 3 | 4)
            }
            _ => false,
        };
        if !valid_version {
            return Err(
                "Application invocation status and version lineage are inconsistent".into(),
            );
        }
        Ok(())
    }

    fn transition(
        &self,
        status: ApplicationInvocationStatus,
        workflow_run_id: Option<WorkflowRunId>,
        occurred_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let occurred_at = canonical_timestamp(occurred_at);
        if occurred_at < self.updated_at {
            return Err("Application invocation transition time regressed".into());
        }
        let mut value = self.clone();
        value.status = status;
        value.workflow_run_id = workflow_run_id;
        value.aggregate_version = self
            .aggregate_version
            .checked_add(1)
            .ok_or_else(|| "Application invocation aggregate version is exhausted".to_owned())?;
        value.updated_at = occurred_at;
        value.completed_at = status.is_terminal().then_some(occurred_at);
        value.validate()?;
        Ok(value)
    }
}

fn application_invocation_input_digest(value: &Value) -> Result<Sha256Digest, String> {
    if !value.is_object() {
        return Err("Application invocation input must be a JSON object".into());
    }
    Ok(Sha256Digest::from_bytes(&canonical_json_bounded(
        value,
        APPLICATION_INVOCATION_INPUT_MAX_BYTES,
        "Application invocation input",
    )?))
}
