use super::{
    DockerfileBuildRecipeInput, ResolveExternalSourceRevision, ResolveExternalSourceRevisionResult,
};
use crate::modules::projects::domain::repositories::IEnvironmentRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{IdempotencyRequest, SourceRevisionId};
use crate::modules::sources::domain::{
    AcceptSourceRevision, BuildRecipe, ExternalSourceRevision, GitProvider, GitReference,
    GitRepository, ISourceResolver, ISourceRevisionRepository, NewExternalSourceRevision,
    SourceRepositoryPolicy, SourceResolutionError, SourceResolutionRequest, SourceRevisionAccepted,
    WebhookDeliveryId, WebhookDeliveryReservation,
};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use serde::Serialize;
use std::sync::Arc;

pub struct ResolveExternalSourceRevisionHandler {
    environments: Arc<dyn IEnvironmentRepository>,
    sources: Arc<dyn ISourceRevisionRepository>,
    resolver: Arc<dyn ISourceResolver>,
    policy: Arc<SourceRepositoryPolicy>,
}

impl ResolveExternalSourceRevisionHandler {
    pub fn new(
        environments: Arc<dyn IEnvironmentRepository>,
        sources: Arc<dyn ISourceRevisionRepository>,
        resolver: Arc<dyn ISourceResolver>,
        policy: Arc<SourceRepositoryPolicy>,
    ) -> Self {
        Self {
            environments,
            sources,
            resolver,
            policy,
        }
    }
}

impl CommandHandler<ResolveExternalSourceRevision> for ResolveExternalSourceRevisionHandler {
    fn execute(
        &self,
        command: ResolveExternalSourceRevision,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<ResolveExternalSourceRevisionResult>>,
    > {
        let environments = Arc::clone(&self.environments);
        let sources = Arc::clone(&self.sources);
        let resolver = Arc::clone(&self.resolver);
        let policy = Arc::clone(&self.policy);
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
            if let Err(error) = policy.require(&repository) {
                return Ok(Err(ApplicationError::Forbidden(error)));
            }
            let reference =
                match GitReference::parse(&command.reference_kind, command.reference_value) {
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
            let canonical = serde_json::to_vec(&CanonicalResolution {
                organization_id: command.organization_id,
                project_id: command.project_id,
                environment_id: command.environment_id,
                repository_identity: repository.identity(),
                reference: &reference,
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
            match sources.replay_acceptance(&idempotency).await {
                Ok(Some(revision)) => {
                    return Ok(Ok(ResolveExternalSourceRevisionResult {
                        revision,
                        replayed: true,
                    }))
                }
                Ok(None) => {}
                Err(error) => return Ok(Err(error.into())),
            }
            let resolved = match resolver
                .resolve(&SourceResolutionRequest {
                    repository: repository.clone(),
                    reference,
                })
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(resolution_error(error))),
            };
            if resolved.repository != repository {
                return Ok(Err(ApplicationError::Internal(
                    "source resolver returned a different repository identity".into(),
                )));
            }
            let revision = ExternalSourceRevision::accept(NewExternalSourceRevision {
                organization_id: command.organization_id,
                project_id: command.project_id,
                environment_id: command.environment_id,
                id: SourceRevisionId::new(),
                repository: resolved.repository,
                commit_sha: resolved.commit_sha,
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
            Ok(Ok(ResolveExternalSourceRevisionResult {
                revision: result.value,
                replayed: result.replayed,
            }))
        })
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CanonicalResolution<'a> {
    organization_id: crate::modules::shared_kernel::domain::OrganizationId,
    project_id: crate::modules::shared_kernel::domain::ProjectId,
    environment_id: crate::modules::shared_kernel::domain::EnvironmentId,
    repository_identity: &'a str,
    reference: &'a GitReference,
    recipe: &'a BuildRecipe,
    webhook_delivery_id: Option<&'a str>,
}

fn resolution_error(error: SourceResolutionError) -> ApplicationError {
    match error {
        SourceResolutionError::Unavailable => {
            ApplicationError::NotFound("source repository or reference is unavailable".into())
        }
        SourceResolutionError::ProviderUnavailable(message)
        | SourceResolutionError::Protocol(message) => ApplicationError::Internal(message),
    }
}
