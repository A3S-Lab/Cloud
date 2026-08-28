use super::ApplicationInvocation;
use crate::modules::shared_kernel::domain::{
    ApplicationId, ApplicationInvocationId, ApplicationReleaseId, ApplicationSessionId,
    EnvironmentId, OntologyId, OntologyRevisionId, OrganizationId, PrincipalId, ProjectId,
    Sha256Digest,
};
use chrono::Duration;
use serde::{Deserialize, Serialize};

/// Immutable authority needed to compose one Application invocation into its
/// ordinary WorkflowRun.
///
/// The invocation owns input and delivery correlation. This companion record
/// retains only the exact external revisions and caller authority that would
/// otherwise be lost between accepting the invocation and creating its run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplicationInvocationWorkflowAuthority {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub application_release_id: ApplicationReleaseId,
    pub application_release_digest: Sha256Digest,
    pub session_id: ApplicationSessionId,
    pub invocation_id: ApplicationInvocationId,
    pub ontology_id: OntologyId,
    pub ontology_revision_id: OntologyRevisionId,
    pub ontology_digest: Sha256Digest,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment_id: Option<EnvironmentId>,
    pub requested_by: PrincipalId,
    pub timeout_seconds: u64,
}

impl ApplicationInvocationWorkflowAuthority {
    pub fn new(
        invocation: &ApplicationInvocation,
        ontology_id: OntologyId,
        ontology_revision_id: OntologyRevisionId,
        ontology_digest: Sha256Digest,
        environment_id: Option<EnvironmentId>,
        requested_by: PrincipalId,
        timeout_seconds: u64,
    ) -> Result<Self, String> {
        invocation.validate()?;
        let value = Self {
            organization_id: invocation.organization_id,
            project_id: invocation.project_id,
            application_id: invocation.application_id,
            application_release_id: invocation.application_release_id,
            application_release_digest: invocation.application_release_digest.clone(),
            session_id: invocation.session_id,
            invocation_id: invocation.id,
            ontology_id,
            ontology_revision_id,
            ontology_digest,
            environment_id,
            requested_by,
            timeout_seconds,
        };
        value.validate_against(invocation)?;
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
            || Sha256Digest::parse(self.application_release_digest.as_str())?
                != self.application_release_digest
            || Sha256Digest::parse(self.ontology_digest.as_str())? != self.ontology_digest
        {
            return Err("stored Application invocation Workflow authority is invalid".into());
        }
        self.timeout()?;
        Ok(())
    }

    pub fn validate_against(&self, invocation: &ApplicationInvocation) -> Result<(), String> {
        self.validate()?;
        invocation.validate()?;
        if self.organization_id != invocation.organization_id
            || self.project_id != invocation.project_id
            || self.application_id != invocation.application_id
            || self.application_release_id != invocation.application_release_id
            || self.application_release_digest != invocation.application_release_digest
            || self.session_id != invocation.session_id
            || self.invocation_id != invocation.id
        {
            return Err(
                "Application invocation Workflow authority changed its immutable owner".into(),
            );
        }
        let timeout = self.timeout()?;
        invocation
            .requested_at
            .checked_add_signed(timeout)
            .ok_or_else(|| {
                "Application invocation Workflow authority deadline overflowed".to_owned()
            })?;
        Ok(())
    }

    fn timeout(&self) -> Result<Duration, String> {
        let timeout_seconds = i64::try_from(self.timeout_seconds).map_err(|_| {
            "Application invocation Workflow authority timeout exceeds supported time".to_owned()
        })?;
        Duration::try_seconds(timeout_seconds).ok_or_else(|| {
            "Application invocation Workflow authority timeout exceeds supported time".to_owned()
        })
    }
}
