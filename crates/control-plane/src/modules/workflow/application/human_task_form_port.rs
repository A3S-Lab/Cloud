use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{
    FormId, FormReleaseId, OrganizationId, ProjectId, Sha256Digest,
};
use a3s_form_core::{CanonicalValue, FormReleaseRef};
use async_trait::async_trait;

#[derive(Debug, Clone, PartialEq)]
pub struct HumanTaskFormEvaluation {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub form_release: FormReleaseRef,
    pub candidate: CanonicalValue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanTaskFormReleaseAuthority {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub form_id: FormId,
    pub form_release_id: FormReleaseId,
    pub form_release_digest: Sha256Digest,
}

impl HumanTaskFormReleaseAuthority {
    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.form_id.as_uuid().is_nil()
            || self.form_release_id.as_uuid().is_nil()
        {
            return Err("HumanTask Form release authority is invalid".into());
        }
        Ok(())
    }
}

impl HumanTaskFormEvaluation {
    pub fn validate(&self) -> Result<(), String> {
        self.form_release
            .validate()
            .map_err(|error| format!("HumanTask Form release is invalid: {error}"))?;
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.form_release.organization_id != self.organization_id.to_string()
            || self.form_release.project_id != self.project_id.to_string()
        {
            return Err("HumanTask Form evaluation authority is invalid".into());
        }
        Ok(())
    }
}

/// Consumer-owned boundary for exact Form release resolution and evaluation.
/// Workflow owns HumanTask lifecycle and accepted evidence; Forms owns only
/// immutable definitions/releases and the version-pinned semantic evaluator.
#[async_trait]
pub trait IHumanTaskFormPort: Send + Sync {
    async fn resolve_interaction_release(
        &self,
        authority: &HumanTaskFormReleaseAuthority,
    ) -> ApplicationResult<FormReleaseRef>;

    async fn evaluate_submission(
        &self,
        request: &HumanTaskFormEvaluation,
    ) -> ApplicationResult<CanonicalValue>;
}
