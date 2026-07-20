use super::{
    AcceptExternalSourceRevision, AcceptExternalSourceRevisionResult, DockerfileBuildRecipeInput,
};
use crate::modules::projects::domain::repositories::IEnvironmentRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{IdempotencyRequest, SourceRevisionId};
use crate::modules::sources::domain::{
    AcceptSourceRevision, BuildRecipe, ExternalSourceRevision, GitCommitSha, GitProvider,
    GitRepository, ISourceRevisionRepository, NewExternalSourceRevision, SourceRevisionAccepted,
    WebhookDeliveryId, WebhookDeliveryReservation,
};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use serde::Serialize;
use std::sync::Arc;

pub struct AcceptExternalSourceRevisionHandler {
    environments: Arc<dyn IEnvironmentRepository>,
    sources: Arc<dyn ISourceRevisionRepository>,
}

impl AcceptExternalSourceRevisionHandler {
    pub fn new(
        environments: Arc<dyn IEnvironmentRepository>,
        sources: Arc<dyn ISourceRevisionRepository>,
    ) -> Self {
        Self {
            environments,
            sources,
        }
    }
}

impl CommandHandler<AcceptExternalSourceRevision> for AcceptExternalSourceRevisionHandler {
    fn execute(
        &self,
        command: AcceptExternalSourceRevision,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<AcceptExternalSourceRevisionResult>>,
    > {
        let environments = Arc::clone(&self.environments);
        let sources = Arc::clone(&self.sources);
        Box::pin(async move {
            match environments
                .find(
                    command.organization_id,
                    command.project_id,
                    command.environment_id,
                )
                .await
            {
                Ok(Some(_)) => {}
                Ok(None) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "environment not found in organization and project".into(),
                    )))
                }
                Err(error) => return Ok(Err(error.into())),
            }
            let provider = match GitProvider::parse(&command.repository_provider) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let repository = match GitRepository::parse(provider, &command.repository_url) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let commit_sha = match GitCommitSha::parse(command.commit_sha) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let DockerfileBuildRecipeInput {
                schema,
                kind,
                context_path,
                dockerfile_path,
                target,
                platforms,
            } = command.recipe;
            let recipe = match BuildRecipe::dockerfile(
                &schema,
                &kind,
                &context_path,
                &dockerfile_path,
                target.as_deref(),
                platforms,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let webhook_delivery_id = match command.webhook_delivery_id {
                Some(value) => match WebhookDeliveryId::parse(value) {
                    Ok(value) => Some(value),
                    Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
                },
                None => None,
            };
            let canonical = serde_json::to_vec(&CanonicalAcceptance {
                organization_id: command.organization_id,
                project_id: command.project_id,
                environment_id: command.environment_id,
                repository_identity: repository.identity(),
                commit_sha: commit_sha.as_str(),
                recipe: &recipe,
                webhook_delivery_id: webhook_delivery_id.as_ref().map(WebhookDeliveryId::as_str),
            })
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/projects/{}/environments/{}/source-revisions",
                    command.organization_id, command.project_id, command.environment_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let revision = ExternalSourceRevision::accept(NewExternalSourceRevision {
                organization_id: command.organization_id,
                project_id: command.project_id,
                environment_id: command.environment_id,
                id: SourceRevisionId::new(),
                repository,
                commit_sha,
                recipe,
                accepted_at: command.accepted_at,
            })
            .map_err(BootError::Internal)?;
            let webhook_delivery =
                webhook_delivery_id.map(|delivery_id| WebhookDeliveryReservation {
                    organization_id: revision.organization_id,
                    provider: revision.repository.provider(),
                    delivery_id,
                    source_identity_digest: revision.source_identity_digest(),
                    received_at: revision.accepted_at,
                });
            let event = SourceRevisionAccepted::envelope(&revision, command.request_id)
                .map_err(|error| BootError::Internal(error.to_string()))?;
            let result = match sources
                .accept(AcceptSourceRevision {
                    revision,
                    webhook_delivery,
                    idempotency,
                    event,
                })
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            Ok(Ok(AcceptExternalSourceRevisionResult {
                revision: result.value,
                replayed: result.replayed,
            }))
        })
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CanonicalAcceptance<'a> {
    organization_id: crate::modules::shared_kernel::domain::OrganizationId,
    project_id: crate::modules::shared_kernel::domain::ProjectId,
    environment_id: crate::modules::shared_kernel::domain::EnvironmentId,
    repository_identity: &'a str,
    commit_sha: &'a str,
    recipe: &'a BuildRecipe,
    webhook_delivery_id: Option<&'a str>,
}
