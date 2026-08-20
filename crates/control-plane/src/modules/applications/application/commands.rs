use super::resource_access::{application_not_found, project};
use super::{ApplicationMutationResult, IApplicationWorkflowRevisionPort};
use crate::modules::applications::domain::{
    Application, ApplicationRecord, ApplicationRelease, ApplicationReleaseContract,
    ApplicationReleasePublished, CreateApplicationWrite, IApplicationRepository,
    PublishApplicationReleaseWrite,
};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    ApplicationId, ApplicationReleaseId, IdempotencyRequest, OrganizationId, PrincipalId,
    ProjectId, RepositoryError, ResourceName,
};
use a3s_boot::{BootError, Command, CommandHandler, CqrsContext};
use chrono::Utc;
use serde::Serialize;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CreateApplication {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub name: String,
    pub description: String,
    pub release_acl: String,
    pub actor_principal_id: PrincipalId,
    pub resource_access: ResourceAccessEvaluator,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for CreateApplication {
    type Output = ApplicationResult<ApplicationMutationResult>;
}

pub struct CreateApplicationHandler {
    applications: Arc<dyn IApplicationRepository>,
    workflows: Arc<dyn IApplicationWorkflowRevisionPort>,
}

impl CreateApplicationHandler {
    pub fn new(
        applications: Arc<dyn IApplicationRepository>,
        workflows: Arc<dyn IApplicationWorkflowRevisionPort>,
    ) -> Self {
        Self {
            applications,
            workflows,
        }
    }
}

impl CommandHandler<CreateApplication> for CreateApplicationHandler {
    fn execute(
        &self,
        command: CreateApplication,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<ApplicationMutationResult>>>
    {
        let applications = Arc::clone(&self.applications);
        let workflows = Arc::clone(&self.workflows);
        Box::pin(async move {
            if let Err(error) = project(command.project_id, &command.resource_access) {
                return Ok(Err(error));
            }
            let name = match ResourceName::parse(command.name) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let contract = match ApplicationReleaseContract::parse_acl(&command.release_acl) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let canonical = serde_json::to_vec(&CanonicalCreateApplication {
                organization_id: command.organization_id,
                project_id: command.project_id,
                name: name.as_str(),
                description: &command.description,
                release_digest: contract.digest().as_str(),
            })
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/projects/{}/applications",
                    command.organization_id, command.project_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            match applications.replay_write(&idempotency).await {
                Ok(Some(record)) => {
                    if !create_replay_matches(
                        &record,
                        command.organization_id,
                        command.project_id,
                        &name,
                        &command.description,
                        &contract,
                    ) {
                        return Err(BootError::Internal(
                            "Application create replay reference is inconsistent".into(),
                        ));
                    }
                    return Ok(Ok(ApplicationMutationResult {
                        record,
                        replayed: true,
                    }));
                }
                Ok(None) => {}
                Err(error) => return Ok(Err(error.into())),
            }
            if let Err(error) = admit_contract(
                workflows.as_ref(),
                command.organization_id,
                command.project_id,
                &contract,
            )
            .await
            {
                return Ok(Err(error));
            }
            let application_id = ApplicationId::new();
            let release = match ApplicationRelease::initial(
                command.organization_id,
                command.project_id,
                application_id,
                ApplicationReleaseId::new(),
                contract,
                command.actor_principal_id,
                Utc::now(),
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let application =
                match Application::create(application_id, name, command.description, &release) {
                    Ok(value) => value,
                    Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
                };
            let event =
                ApplicationReleasePublished::published(&application, &release, command.request_id)
                    .map_err(BootError::Internal)?;
            let record =
                ApplicationRecord::new(application, release).map_err(BootError::Internal)?;
            match applications
                .create(CreateApplicationWrite {
                    record,
                    event,
                    actor_principal_id: command.actor_principal_id,
                    request_id: command.request_id,
                    idempotency,
                })
                .await
            {
                Ok(result) => Ok(Ok(ApplicationMutationResult {
                    record: result.value,
                    replayed: result.replayed,
                })),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

#[derive(Debug, Clone)]
pub struct PublishApplicationRelease {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub expected_version: u64,
    pub release_acl: String,
    pub actor_principal_id: PrincipalId,
    pub resource_access: ResourceAccessEvaluator,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for PublishApplicationRelease {
    type Output = ApplicationResult<ApplicationMutationResult>;
}

pub struct PublishApplicationReleaseHandler {
    applications: Arc<dyn IApplicationRepository>,
    workflows: Arc<dyn IApplicationWorkflowRevisionPort>,
}

impl PublishApplicationReleaseHandler {
    pub fn new(
        applications: Arc<dyn IApplicationRepository>,
        workflows: Arc<dyn IApplicationWorkflowRevisionPort>,
    ) -> Self {
        Self {
            applications,
            workflows,
        }
    }
}

impl CommandHandler<PublishApplicationRelease> for PublishApplicationReleaseHandler {
    fn execute(
        &self,
        command: PublishApplicationRelease,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<ApplicationMutationResult>>>
    {
        let applications = Arc::clone(&self.applications);
        let workflows = Arc::clone(&self.workflows);
        Box::pin(async move {
            if let Err(error) = project(command.project_id, &command.resource_access) {
                return Ok(Err(error));
            }
            if command.expected_version == 0 {
                return Ok(Err(ApplicationError::Invalid(
                    "expected Application version must be positive".into(),
                )));
            }
            let contract = match ApplicationReleaseContract::parse_acl(&command.release_acl) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let canonical = serde_json::to_vec(&CanonicalPublishApplicationRelease {
                organization_id: command.organization_id,
                project_id: command.project_id,
                application_id: command.application_id,
                expected_version: command.expected_version,
                release_digest: contract.digest().as_str(),
            })
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/projects/{}/applications/{}/releases",
                    command.organization_id, command.project_id, command.application_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            match applications.replay_write(&idempotency).await {
                Ok(Some(record)) => {
                    if !publish_replay_matches(
                        &record,
                        command.organization_id,
                        command.project_id,
                        command.application_id,
                        command.expected_version,
                        &contract,
                    ) {
                        return Err(BootError::Internal(
                            "Application publication replay reference is inconsistent".into(),
                        ));
                    }
                    return Ok(Ok(ApplicationMutationResult {
                        record,
                        replayed: true,
                    }));
                }
                Ok(None) => {}
                Err(error) => return Ok(Err(error.into())),
            }
            let current = match applications
                .find(
                    command.organization_id,
                    command.project_id,
                    command.application_id,
                )
                .await
            {
                Ok(Some(value)) => value,
                Ok(None) | Err(RepositoryError::NotFound) => {
                    return Ok(Err(application_not_found()))
                }
                Err(error) => return Ok(Err(error.into())),
            };
            if current.aggregate_version != command.expected_version {
                return Ok(Err(ApplicationError::Conflict(
                    "Application release was published from a stale aggregate version".into(),
                )));
            }
            let parent = match applications
                .find_release(
                    command.organization_id,
                    command.project_id,
                    command.application_id,
                    current.current_release_id,
                )
                .await
            {
                Ok(Some(value)) => value,
                Ok(None) | Err(RepositoryError::NotFound) => {
                    return Err(BootError::Internal(
                        "Application current release is missing".into(),
                    ))
                }
                Err(error) => return Ok(Err(error.into())),
            };
            if let Err(error) = admit_contract(
                workflows.as_ref(),
                command.organization_id,
                command.project_id,
                &contract,
            )
            .await
            {
                return Ok(Err(error));
            }
            let release = match ApplicationRelease::successor(
                &parent,
                ApplicationReleaseId::new(),
                contract,
                command.actor_principal_id,
                Utc::now(),
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let application = match current.advance(command.expected_version, &release) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Conflict(error))),
            };
            let event =
                ApplicationReleasePublished::published(&application, &release, command.request_id)
                    .map_err(BootError::Internal)?;
            let record =
                ApplicationRecord::new(application, release).map_err(BootError::Internal)?;
            match applications
                .publish_release(PublishApplicationReleaseWrite {
                    record,
                    expected_version: command.expected_version,
                    event,
                    actor_principal_id: command.actor_principal_id,
                    request_id: command.request_id,
                    idempotency,
                })
                .await
            {
                Ok(result) => Ok(Ok(ApplicationMutationResult {
                    record: result.value,
                    replayed: result.replayed,
                })),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

async fn admit_contract(
    workflows: &dyn IApplicationWorkflowRevisionPort,
    organization_id: OrganizationId,
    project_id: ProjectId,
    contract: &ApplicationReleaseContract,
) -> ApplicationResult<()> {
    let binding = &contract.spec().workflow;
    let evidence = workflows
        .resolve_revision(
            organization_id,
            project_id,
            binding.workflow_definition_id,
            binding.workflow_revision_id,
        )
        .await?;
    contract
        .validate_workflow_evidence(organization_id, project_id, &evidence)
        .map_err(ApplicationError::Conflict)
}

fn create_replay_matches(
    record: &ApplicationRecord,
    organization_id: OrganizationId,
    project_id: ProjectId,
    name: &ResourceName,
    description: &str,
    contract: &ApplicationReleaseContract,
) -> bool {
    record.application.organization_id == organization_id
        && record.application.project_id == project_id
        && &record.application.name == name
        && record.application.description == description
        && record.release.contract.digest() == contract.digest()
}

fn publish_replay_matches(
    record: &ApplicationRecord,
    organization_id: OrganizationId,
    project_id: ProjectId,
    application_id: ApplicationId,
    expected_version: u64,
    contract: &ApplicationReleaseContract,
) -> bool {
    record.application.organization_id == organization_id
        && record.application.project_id == project_id
        && record.application.id == application_id
        && record.application.aggregate_version == expected_version.saturating_add(1)
        && record.release.contract.digest() == contract.digest()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CanonicalCreateApplication<'a> {
    organization_id: OrganizationId,
    project_id: ProjectId,
    name: &'a str,
    description: &'a str,
    release_digest: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CanonicalPublishApplicationRelease<'a> {
    organization_id: OrganizationId,
    project_id: ProjectId,
    application_id: ApplicationId,
    expected_version: u64,
    release_digest: &'a str,
}
