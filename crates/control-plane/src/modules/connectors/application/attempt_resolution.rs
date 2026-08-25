use super::resource_access::{attempt_not_found, attempt_resolution_not_found, environment};
use crate::modules::connectors::domain::{
    normalize_connector_execution_attempt_resolution_reason, ConnectorExecutionAttemptResolution,
    ConnectorExecutionAttemptResolved, IConnectorExecutionAttemptRepository,
    IConnectorExecutionAttemptResolutionRepository, ResolveConnectorExecutionAttemptWrite,
};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    ConnectorProfileId, ConnectorRevisionId, EnvironmentId, IdempotencyRequest, OrganizationId,
    PrincipalId, ProjectId, RepositoryError,
};
use a3s_boot::{BootError, Command, CommandHandler, CqrsContext, Query, QueryHandler};
use chrono::Utc;
use serde::Serialize;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ResolveConnectorExecutionAttempt {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub profile_id: ConnectorProfileId,
    pub revision_id: ConnectorRevisionId,
    pub attempt_id: Uuid,
    pub reason: String,
    pub actor_principal_id: PrincipalId,
    pub resource_access: ResourceAccessEvaluator,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for ResolveConnectorExecutionAttempt {
    type Output = ApplicationResult<ConnectorExecutionAttemptResolutionMutationResult>;
}

pub struct ResolveConnectorExecutionAttemptHandler {
    attempts: Arc<dyn IConnectorExecutionAttemptRepository>,
    resolutions: Arc<dyn IConnectorExecutionAttemptResolutionRepository>,
}

impl ResolveConnectorExecutionAttemptHandler {
    pub fn new(
        attempts: Arc<dyn IConnectorExecutionAttemptRepository>,
        resolutions: Arc<dyn IConnectorExecutionAttemptResolutionRepository>,
    ) -> Self {
        Self {
            attempts,
            resolutions,
        }
    }
}

impl CommandHandler<ResolveConnectorExecutionAttempt> for ResolveConnectorExecutionAttemptHandler {
    fn execute(
        &self,
        command: ResolveConnectorExecutionAttempt,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<ConnectorExecutionAttemptResolutionMutationResult>>,
    > {
        let attempts = Arc::clone(&self.attempts);
        let resolutions = Arc::clone(&self.resolutions);
        Box::pin(async move {
            if let Err(error) = environment(
                command.project_id,
                command.environment_id,
                &command.resource_access,
            ) {
                return Ok(Err(error));
            }
            if command.attempt_id.is_nil() {
                return Ok(Err(ApplicationError::Invalid(
                    "Connector execution attempt identity is invalid".into(),
                )));
            }
            let reason = match normalize_connector_execution_attempt_resolution_reason(
                command.reason.as_str(),
            ) {
                Ok(reason) => reason,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let canonical = serde_json::to_vec(&CanonicalResolveConnectorExecutionAttempt {
                organization_id: command.organization_id,
                project_id: command.project_id,
                environment_id: command.environment_id,
                profile_id: command.profile_id,
                revision_id: command.revision_id,
                attempt_id: command.attempt_id,
                resolution: "indeterminate",
                reason: &reason,
            })
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "connector-execution-attempt-resolution/{}/{}",
                    command.organization_id, command.attempt_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(idempotency) => idempotency,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            match resolutions.replay_resolution_write(&idempotency).await {
                Ok(Some(resolution)) => {
                    if !matches_command(
                        &resolution,
                        command.organization_id,
                        command.project_id,
                        command.environment_id,
                        command.profile_id,
                        command.revision_id,
                        command.attempt_id,
                        &reason,
                    ) {
                        return Err(BootError::Internal(
                            "Connector attempt resolution replay authority is inconsistent".into(),
                        ));
                    }
                    return Ok(Ok(ConnectorExecutionAttemptResolutionMutationResult {
                        resolution,
                        replayed: true,
                    }));
                }
                Ok(None) => {}
                Err(error) => return Ok(Err(error.into())),
            }
            let attempt = match attempts
                .find(
                    command.organization_id,
                    command.project_id,
                    command.environment_id,
                    command.profile_id,
                    command.revision_id,
                    command.attempt_id,
                )
                .await
            {
                Ok(Some(record)) => record.attempt,
                Ok(None) | Err(RepositoryError::NotFound) => return Ok(Err(attempt_not_found())),
                Err(error) => return Ok(Err(error.into())),
            };
            let (resolution, evidence) = match ConnectorExecutionAttemptResolution::new(
                &attempt,
                reason,
                command.actor_principal_id,
                Utc::now(),
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Conflict(error))),
            };
            let event =
                ConnectorExecutionAttemptResolved::envelope(&resolution, command.request_id)
                    .map_err(|error| BootError::Internal(error.to_string()))?;
            match resolutions
                .resolve_indeterminate(ResolveConnectorExecutionAttemptWrite {
                    resolution,
                    evidence,
                    event,
                    actor_principal_id: command.actor_principal_id,
                    request_id: command.request_id,
                    idempotency,
                })
                .await
            {
                Ok(write) => Ok(Ok(ConnectorExecutionAttemptResolutionMutationResult {
                    resolution: write.value,
                    replayed: write.replayed,
                })),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

#[derive(Debug, Clone)]
pub struct GetConnectorExecutionAttemptResolution {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub profile_id: ConnectorProfileId,
    pub revision_id: ConnectorRevisionId,
    pub attempt_id: Uuid,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for GetConnectorExecutionAttemptResolution {
    type Output = ApplicationResult<ConnectorExecutionAttemptResolution>;
}

pub struct GetConnectorExecutionAttemptResolutionHandler {
    resolutions: Arc<dyn IConnectorExecutionAttemptResolutionRepository>,
}

impl GetConnectorExecutionAttemptResolutionHandler {
    pub fn new(resolutions: Arc<dyn IConnectorExecutionAttemptResolutionRepository>) -> Self {
        Self { resolutions }
    }
}

impl QueryHandler<GetConnectorExecutionAttemptResolution>
    for GetConnectorExecutionAttemptResolutionHandler
{
    fn execute(
        &self,
        query: GetConnectorExecutionAttemptResolution,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<ConnectorExecutionAttemptResolution>>,
    > {
        let resolutions = Arc::clone(&self.resolutions);
        Box::pin(async move {
            if let Err(error) = environment(
                query.project_id,
                query.environment_id,
                &query.resource_access,
            ) {
                return Ok(Err(error));
            }
            if query.attempt_id.is_nil() {
                return Ok(Err(ApplicationError::Invalid(
                    "Connector execution attempt identity is invalid".into(),
                )));
            }
            Ok(
                match resolutions
                    .find_resolution(
                        query.organization_id,
                        query.project_id,
                        query.environment_id,
                        query.profile_id,
                        query.revision_id,
                        query.attempt_id,
                    )
                    .await
                {
                    Ok(Some(resolution)) => Ok(resolution),
                    Ok(None) | Err(RepositoryError::NotFound) => {
                        Err(attempt_resolution_not_found())
                    }
                    Err(error) => Err(error.into()),
                },
            )
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorExecutionAttemptResolutionMutationResult {
    pub resolution: ConnectorExecutionAttemptResolution,
    pub replayed: bool,
}

#[allow(clippy::too_many_arguments)]
fn matches_command(
    resolution: &ConnectorExecutionAttemptResolution,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    profile_id: ConnectorProfileId,
    revision_id: ConnectorRevisionId,
    attempt_id: Uuid,
    reason: &str,
) -> bool {
    let binding = resolution.binding();
    binding.organization_id() == organization_id
        && binding.project_id() == project_id
        && binding.environment_id() == environment_id
        && binding.profile_id() == profile_id
        && binding.revision_id() == revision_id
        && binding.attempt_id() == attempt_id
        && resolution.reason() == reason
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CanonicalResolveConnectorExecutionAttempt<'a> {
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    profile_id: ConnectorProfileId,
    revision_id: ConnectorRevisionId,
    attempt_id: Uuid,
    resolution: &'static str,
    reason: &'a str,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::connectors::domain::{
        BeginConnectorExecutionDispatch, ConnectorDefinition, ConnectorExecutionAttemptBinding,
        ConnectorExecutionRequest, ConnectorExecutionReservation, ConnectorHttpAuthentication,
        ConnectorHttpDefinition, ConnectorHttpDefinitionSpec, ConnectorHttpDestination,
        ConnectorHttpMethod, ConnectorHttpStatusPolicy, ConnectorRevision,
        IConnectorExecutionAttemptRepository, ReserveConnectorExecutionAttempt,
    };
    use crate::modules::connectors::infrastructure::InMemoryConnectorExecutionRepository;
    use crate::modules::identity::domain::value_objects::ResourceGrantScope;
    use crate::modules::shared_kernel::domain::{
        canonical_timestamp, ConnectorProfileId, ConnectorRevisionId, EnvironmentId,
        OrganizationId, ProjectId,
    };
    use a3s_boot::ModuleRef;
    use chrono::Duration;

    #[tokio::test]
    async fn resolution_authorizes_before_replay_and_is_exactly_idempotent() {
        let now = canonical_timestamp(Utc::now());
        let reserved_at = now - Duration::seconds(60);
        let revision = ConnectorRevision::initial(
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            ConnectorProfileId::new(),
            ConnectorRevisionId::new(),
            ConnectorDefinition::Http(
                ConnectorHttpDefinition::from_spec(ConnectorHttpDefinitionSpec {
                    destination: ConnectorHttpDestination::LiteralHttps {
                        endpoint: "https://hooks.example.test/attempt-resolution".into(),
                    },
                    method: ConnectorHttpMethod::Post,
                    request_content_type: "application/json".into(),
                    maximum_request_bytes: 1024,
                    maximum_response_bytes: 1024,
                    timeout_milliseconds: 1_000,
                    status_policy: ConnectorHttpStatusPolicy::standard_webhook(),
                    authentication: ConnectorHttpAuthentication::None,
                })
                .expect("definition"),
            ),
            PrincipalId::new(),
            reserved_at,
        )
        .expect("revision");
        let request = ConnectorExecutionRequest::new(
            revision.id,
            Uuid::now_v7(),
            "application/json",
            b"bounded recovery".to_vec(),
        )
        .expect("request");
        let repository = Arc::new(InMemoryConnectorExecutionRepository::new());
        let fence = match repository
            .reserve(
                ReserveConnectorExecutionAttempt::new(
                    ConnectorExecutionAttemptBinding::from_exact(&revision, &request)
                        .expect("binding"),
                    Uuid::now_v7(),
                    reserved_at,
                    reserved_at + Duration::seconds(30),
                )
                .expect("reservation"),
            )
            .await
            .expect("reserve")
        {
            ConnectorExecutionReservation::Acquired { fence, .. } => fence,
            other => panic!("unexpected reservation: {other:?}"),
        };
        repository
            .begin_dispatch(
                BeginConnectorExecutionDispatch::new(
                    fence,
                    reserved_at + Duration::seconds(1),
                    reserved_at + Duration::seconds(5),
                )
                .expect("dispatch"),
            )
            .await
            .expect("begin dispatch");

        let handler =
            ResolveConnectorExecutionAttemptHandler::new(repository.clone(), repository.clone());
        let command = ResolveConnectorExecutionAttempt {
            organization_id: revision.organization_id,
            project_id: revision.project_id,
            environment_id: revision.environment_id,
            profile_id: revision.profile_id,
            revision_id: revision.id,
            attempt_id: request.attempt_id(),
            reason: "  provider outcome unavailable  ".into(),
            actor_principal_id: PrincipalId::new(),
            resource_access: ResourceAccessEvaluator::organization_wide(),
            idempotency_key: "resolve-exact-attempt".into(),
            request_id: Uuid::now_v7(),
        };
        let first = handler
            .execute(command.clone(), context())
            .await
            .expect("command framework")
            .expect("resolution");
        assert!(!first.replayed);
        assert_eq!(first.resolution.reason(), "provider outcome unavailable");

        let denied = handler
            .execute(
                ResolveConnectorExecutionAttempt {
                    resource_access: ResourceAccessEvaluator::restricted([
                        ResourceGrantScope::Environment {
                            project_id: revision.project_id,
                            environment_id: EnvironmentId::new(),
                        },
                    ]),
                    ..command.clone()
                },
                context(),
            )
            .await
            .expect("command framework");
        assert!(matches!(denied, Err(ApplicationError::NotFound(_))));

        let replay = handler
            .execute(command.clone(), context())
            .await
            .expect("command framework")
            .expect("resolution replay");
        assert!(replay.replayed);
        assert_eq!(replay.resolution, first.resolution);

        let conflict = handler
            .execute(
                ResolveConnectorExecutionAttempt {
                    reason: "changed conclusion".into(),
                    ..command
                },
                context(),
            )
            .await
            .expect("command framework");
        assert!(matches!(conflict, Err(ApplicationError::Conflict(_))));
    }

    fn context() -> CqrsContext {
        CqrsContext::new(ModuleRef::new())
    }
}
