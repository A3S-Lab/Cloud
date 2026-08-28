use crate::modules::applications::domain::{
    ApplicationInvocation, ApplicationInvocationWorkflowAuthority, ApplicationRelease,
    ApplicationSession, ApplicationWorkflowBinding, APPLICATION_INVOCATION_INPUT_MAX_BYTES,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, canonical_timestamp, ApplicationId, ApplicationInvocationId,
    ApplicationReleaseId, ApplicationSessionId, EnvironmentId, OntologyId, OntologyRevisionId,
    OrganizationId, PlanRevisionId, PrincipalId, ProjectId, Sha256Digest, WorkflowGoalId,
    WorkflowRunId,
};
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

const APPLICATION_WORKFLOW_RUN_IDENTITY: &[u8] = b"cloud.application.workflow-run.v1";
const APPLICATION_WORKFLOW_RUN_REQUEST_MAX_BYTES: usize = 128 * 1024;

/// Applications-owned request to the existing WorkflowRun authority.
///
/// The request contains exact immutable identities and digests only. Workflow
/// still owns Goal/Plan compilation, WorkflowRun persistence, Flow dispatch,
/// cancellation, recovery, and history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplicationWorkflowRunRequest {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub application_release_id: ApplicationReleaseId,
    pub application_release_digest: Sha256Digest,
    pub session_id: ApplicationSessionId,
    pub invocation_id: ApplicationInvocationId,
    pub workflow: ApplicationWorkflowBinding,
    pub ontology_id: OntologyId,
    pub ontology_revision_id: OntologyRevisionId,
    pub ontology_digest: Sha256Digest,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment_id: Option<EnvironmentId>,
    pub input: Value,
    pub input_digest: Sha256Digest,
    pub requested_by: PrincipalId,
    pub requested_at: DateTime<Utc>,
    pub timeout_seconds: u64,
}

impl ApplicationWorkflowRunRequest {
    #[allow(clippy::too_many_arguments)]
    pub fn from_invocation(
        release: &ApplicationRelease,
        session: &ApplicationSession,
        invocation: &ApplicationInvocation,
        authority: &ApplicationInvocationWorkflowAuthority,
    ) -> Result<Self, String> {
        session.validate_release(release)?;
        invocation.validate()?;
        authority.validate_against(invocation)?;
        if invocation.organization_id != session.organization_id
            || invocation.project_id != session.project_id
            || invocation.application_id != session.application_id
            || invocation.application_release_id != session.application_release_id
            || invocation.application_release_digest != session.application_release_digest
            || invocation.session_id != session.id
        {
            return Err(
                "Application invocation does not belong to the exact release-pinned session".into(),
            );
        }
        let value = Self {
            organization_id: invocation.organization_id,
            project_id: invocation.project_id,
            application_id: invocation.application_id,
            application_release_id: invocation.application_release_id,
            application_release_digest: invocation.application_release_digest.clone(),
            session_id: invocation.session_id,
            invocation_id: invocation.id,
            workflow: release.contract.spec().workflow.clone(),
            ontology_id: authority.ontology_id,
            ontology_revision_id: authority.ontology_revision_id,
            ontology_digest: authority.ontology_digest.clone(),
            environment_id: authority.environment_id,
            input: invocation.input.clone(),
            input_digest: invocation.input_digest.clone(),
            requested_by: authority.requested_by,
            requested_at: invocation.requested_at,
            timeout_seconds: authority.timeout_seconds,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.application_id.as_uuid().is_nil()
            || self.application_release_id.as_uuid().is_nil()
            || self.session_id.as_uuid().is_nil()
            || self.invocation_id.as_uuid().is_nil()
            || self.ontology_id.as_uuid().is_nil()
            || self.ontology_revision_id.as_uuid().is_nil()
            || self.requested_by.as_uuid().is_nil()
            || self
                .environment_id
                .is_some_and(|environment_id| environment_id.as_uuid().is_nil())
            || self.timeout_seconds == 0
            || self.requested_at != canonical_timestamp(self.requested_at)
        {
            return Err("Application WorkflowRun request authority is invalid".into());
        }
        self.workflow.validate()?;
        for (label, digest) in [
            ("Application release", &self.application_release_digest),
            ("Ontology", &self.ontology_digest),
            ("input", &self.input_digest),
        ] {
            if Sha256Digest::parse(digest.as_str())? != *digest {
                return Err(format!(
                    "Application WorkflowRun {label} digest is not canonical"
                ));
            }
        }
        if !self.input.is_object() {
            return Err("Application WorkflowRun input must be a JSON object".into());
        }
        self.deadline_at()?;
        let canonical_input = canonical_json_bounded(
            &self.input,
            APPLICATION_INVOCATION_INPUT_MAX_BYTES,
            "Application WorkflowRun input",
        )?;
        if Sha256Digest::from_bytes(&canonical_input) != self.input_digest {
            return Err("Application WorkflowRun input digest drifted".into());
        }
        self.canonical_bytes()?;
        Ok(())
    }

    pub(crate) fn deadline_at(&self) -> Result<DateTime<Utc>, String> {
        let timeout_seconds = i64::try_from(self.timeout_seconds)
            .map_err(|_| "Application WorkflowRun timeout exceeds supported time".to_owned())?;
        let timeout = Duration::try_seconds(timeout_seconds)
            .ok_or_else(|| "Application WorkflowRun timeout exceeds supported time".to_owned())?;
        self.requested_at
            .checked_add_signed(timeout)
            .ok_or_else(|| "Application WorkflowRun deadline overflowed".to_owned())
    }

    pub fn workflow_run_id(&self) -> WorkflowRunId {
        let mut identity = Vec::with_capacity(APPLICATION_WORKFLOW_RUN_IDENTITY.len() + 33);
        identity.extend_from_slice(APPLICATION_WORKFLOW_RUN_IDENTITY);
        identity.push(0);
        identity.extend_from_slice(self.application_id.as_uuid().as_bytes());
        identity.extend_from_slice(self.invocation_id.as_uuid().as_bytes());
        WorkflowRunId::from_uuid(Uuid::new_v5(&self.organization_id.as_uuid(), &identity))
    }

    pub fn workflow_goal_id(&self) -> WorkflowGoalId {
        WorkflowGoalId::from_uuid(Uuid::new_v5(&self.workflow_run_id().as_uuid(), b"goal"))
    }

    pub fn plan_revision_id(&self) -> PlanRevisionId {
        PlanRevisionId::from_uuid(Uuid::new_v5(&self.workflow_run_id().as_uuid(), b"plan"))
    }

    pub(crate) fn request_id(&self, purpose: &[u8]) -> Uuid {
        Uuid::new_v5(&self.workflow_run_id().as_uuid(), purpose)
    }

    pub(crate) fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_json_bounded(
            self,
            APPLICATION_WORKFLOW_RUN_REQUEST_MAX_BYTES,
            "Application WorkflowRun request",
        )
    }
}

/// Redacted proof that Workflow adopted the exact Application invocation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplicationWorkflowRunEvidence {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub application_release_id: ApplicationReleaseId,
    pub application_release_digest: Sha256Digest,
    pub session_id: ApplicationSessionId,
    pub invocation_id: ApplicationInvocationId,
    pub workflow_run_id: WorkflowRunId,
    pub workflow_goal_id: WorkflowGoalId,
    pub plan_revision_id: PlanRevisionId,
    pub plan_digest: Sha256Digest,
    pub workflow: ApplicationWorkflowBinding,
    pub ontology_id: OntologyId,
    pub ontology_revision_id: OntologyRevisionId,
    pub ontology_digest: Sha256Digest,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment_id: Option<EnvironmentId>,
    pub input_digest: Sha256Digest,
    pub requested_by: PrincipalId,
    pub requested_at: DateTime<Utc>,
    pub deadline_at: DateTime<Utc>,
}

impl ApplicationWorkflowRunEvidence {
    pub fn validate_against(&self, request: &ApplicationWorkflowRunRequest) -> Result<(), String> {
        request.validate()?;
        self.workflow.validate()?;
        let deadline_at = request.deadline_at()?;
        if self.organization_id != request.organization_id
            || self.project_id != request.project_id
            || self.application_id != request.application_id
            || self.application_release_id != request.application_release_id
            || self.application_release_digest != request.application_release_digest
            || self.session_id != request.session_id
            || self.invocation_id != request.invocation_id
            || self.workflow_run_id != request.workflow_run_id()
            || self.workflow_goal_id != request.workflow_goal_id()
            || self.plan_revision_id != request.plan_revision_id()
            || self.workflow != request.workflow
            || self.ontology_id != request.ontology_id
            || self.ontology_revision_id != request.ontology_revision_id
            || self.ontology_digest != request.ontology_digest
            || self.environment_id != request.environment_id
            || self.input_digest != request.input_digest
            || self.requested_by != request.requested_by
            || self.requested_at != request.requested_at
            || self.deadline_at != deadline_at
            || Sha256Digest::parse(self.plan_digest.as_str())? != self.plan_digest
            || self.deadline_at != canonical_timestamp(self.deadline_at)
        {
            return Err("Application WorkflowRun evidence drifted from its exact request".into());
        }
        Ok(())
    }
}

#[async_trait]
pub trait IApplicationWorkflowRunPort: Send + Sync {
    /// Admit and normalize a caller timeout through Workflow's owning rule.
    /// Applications persists only the returned value and does not copy the
    /// Workflow default or maximum into its own Domain.
    fn admit_timeout_seconds(&self, requested: Option<u64>) -> ApplicationResult<u64>;

    async fn start_or_adopt(
        &self,
        request: &ApplicationWorkflowRunRequest,
    ) -> ApplicationResult<ApplicationWorkflowRunEvidence>;

    async fn request_cancellation(
        &self,
        _request: &ApplicationWorkflowRunRequest,
        _reason: &str,
        _requested_at: DateTime<Utc>,
    ) -> ApplicationResult<Option<ApplicationWorkflowRunEvidence>> {
        Err(ApplicationError::Unavailable(
            "Application WorkflowRun cancellation is not composed".into(),
        ))
    }
}
