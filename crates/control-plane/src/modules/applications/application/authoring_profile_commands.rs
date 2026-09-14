use super::authoring_profile::ApplicationAuthoringProfile;
use super::preset_workflow_port::{
    ApplicationPresetTarget, ApplicationPresetWorkflowResult, IApplicationPresetWorkflowPort,
};
use super::resource_access::project;
use crate::modules::applications::domain::{
    ApplicationAudience, ApplicationDeliveryPolicy, ApplicationExperience,
    ApplicationReleaseContract,
};
use crate::modules::applications::ApplicationAccess;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    ApplicationId, OrganizationId, PrincipalId, ProjectId, Sha256Digest,
};
use a3s_boot::{Command, CommandHandler, CqrsContext};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

/// Publish one preset authoring profile through the C4 wrapper compiler.
#[derive(Debug, Clone)]
pub struct PublishApplicationAuthoringProfile {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub application_release_number: u64,
    pub experience: ApplicationExperience,
    pub audience: ApplicationAudience,
    pub delivery: ApplicationDeliveryPolicy,
    pub target: ApplicationPresetTarget,
    pub presentation_digest: Sha256Digest,
    pub actor_principal_id: PrincipalId,
    pub access: ApplicationAccess,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for PublishApplicationAuthoringProfile {
    type Output = ApplicationResult<ApplicationAuthoringProfilePublication>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplicationAuthoringProfilePublication {
    pub profile: ApplicationAuthoringProfile,
    pub preset: ApplicationPresetWorkflowResult,
    pub release_contract_acl: String,
    pub release_contract_digest: Sha256Digest,
    pub replayed: bool,
}

pub struct PublishApplicationAuthoringProfileHandler {
    workflows: Arc<dyn IApplicationPresetWorkflowPort>,
}

impl PublishApplicationAuthoringProfileHandler {
    pub fn new(workflows: Arc<dyn IApplicationPresetWorkflowPort>) -> Self {
        Self { workflows }
    }
}

impl CommandHandler<PublishApplicationAuthoringProfile>
    for PublishApplicationAuthoringProfileHandler
{
    fn execute(
        &self,
        command: PublishApplicationAuthoringProfile,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<ApplicationAuthoringProfilePublication>>,
    > {
        let workflows = Arc::clone(&self.workflows);
        Box::pin(async move {
            if let Err(error) = project(command.project_id, &command.access) {
                return Ok(Err(error));
            }
            let profile = match ApplicationAuthoringProfile::create(
                command.organization_id,
                command.project_id,
                command.application_id,
                command.application_release_number,
                command.experience,
                command.audience,
                command.delivery,
                command.target,
                command.presentation_digest,
                Utc::now(),
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let request = match profile.to_preset_request(
                command.actor_principal_id,
                command.idempotency_key,
                command.request_id,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let preset = match workflows.compile_and_publish(&request).await {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            let spec = match profile.to_release_contract_spec(preset.evidence.binding.clone()) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let contract = match ApplicationReleaseContract::from_spec(spec) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            Ok(Ok(ApplicationAuthoringProfilePublication {
                profile,
                replayed: preset.replayed,
                release_contract_acl: contract.canonical_acl().to_owned(),
                release_contract_digest: contract.digest().clone(),
                preset,
            }))
        })
    }
}
