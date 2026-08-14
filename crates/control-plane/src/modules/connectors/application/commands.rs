use super::resource_access::{environment, environment_not_found, profile_not_found};
use super::secret_references::validate_definition_secret_references;
use super::ConnectorProfileMutationResult;
use crate::modules::connectors::domain::{
    ConnectorDefinition, ConnectorProfile, ConnectorRecord, ConnectorRevision,
    ConnectorRevisionPublished, CreateConnectorProfileWrite, IConnectorProfileRepository,
    ReviseConnectorProfileWrite,
};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::projects::domain::repositories::IEnvironmentRepository;
use crate::modules::secrets::application::ExactSecretVersionAccess;
use crate::modules::secrets::domain::ISecretRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    ConnectorProfileId, ConnectorRevisionId, EnvironmentId, IdempotencyRequest, OrganizationId,
    PrincipalId, ProjectId, ResourceName,
};
use a3s_boot::{BootError, Command, CommandHandler, CqrsContext};
use chrono::Utc;
use serde::Serialize;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CreateConnectorProfile {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub name: String,
    pub definition_acl: String,
    pub actor_principal_id: PrincipalId,
    pub resource_access: ResourceAccessEvaluator,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for CreateConnectorProfile {
    type Output = ApplicationResult<ConnectorProfileMutationResult>;
}

pub struct CreateConnectorProfileHandler {
    environments: Arc<dyn IEnvironmentRepository>,
    connectors: Arc<dyn IConnectorProfileRepository>,
    secret_access: ExactSecretVersionAccess,
}

impl CreateConnectorProfileHandler {
    pub fn new(
        environments: Arc<dyn IEnvironmentRepository>,
        connectors: Arc<dyn IConnectorProfileRepository>,
        secrets: Arc<dyn ISecretRepository>,
    ) -> Self {
        Self {
            environments,
            connectors,
            secret_access: ExactSecretVersionAccess::new(secrets),
        }
    }
}

impl CommandHandler<CreateConnectorProfile> for CreateConnectorProfileHandler {
    fn execute(
        &self,
        command: CreateConnectorProfile,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<ConnectorProfileMutationResult>>,
    > {
        let environments = Arc::clone(&self.environments);
        let connectors = Arc::clone(&self.connectors);
        let secret_access = self.secret_access.clone();
        Box::pin(async move {
            if let Err(error) = environment(
                command.project_id,
                command.environment_id,
                &command.resource_access,
            ) {
                return Ok(Err(error));
            }
            let name = match ResourceName::parse(command.name) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let definition = match ConnectorDefinition::parse_acl(&command.definition_acl) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let canonical = serde_json::to_vec(&CanonicalCreateConnectorProfile {
                organization_id: command.organization_id,
                project_id: command.project_id,
                environment_id: command.environment_id,
                name: name.as_str(),
                definition_digest: definition.digest().as_str(),
            })
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/projects/{}/environments/{}/connector-profiles",
                    command.organization_id, command.project_id, command.environment_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            match connectors.replay_write(&idempotency).await {
                Ok(Some(record)) => {
                    if !create_replay_matches(
                        &record,
                        command.organization_id,
                        command.project_id,
                        command.environment_id,
                        &name,
                        &definition,
                    ) {
                        return Err(BootError::Internal(
                            "Connector create replay reference is inconsistent".into(),
                        ));
                    }
                    return Ok(Ok(ConnectorProfileMutationResult {
                        record,
                        replayed: true,
                    }));
                }
                Ok(None) => {}
                Err(error) => return Ok(Err(error.into())),
            }
            match environments
                .find(
                    command.organization_id,
                    command.project_id,
                    command.environment_id,
                )
                .await
            {
                Ok(Some(_)) => {}
                Ok(None)
                | Err(crate::modules::shared_kernel::domain::RepositoryError::NotFound) => {
                    return Ok(Err(environment_not_found()))
                }
                Err(error) => return Ok(Err(error.into())),
            }
            if let Err(error) = validate_definition_secret_references(
                &secret_access,
                command.organization_id,
                command.project_id,
                command.environment_id,
                &definition,
            )
            .await
            {
                return Ok(Err(error));
            }
            let profile_id = ConnectorProfileId::new();
            let revision = match ConnectorRevision::initial(
                command.organization_id,
                command.project_id,
                command.environment_id,
                profile_id,
                ConnectorRevisionId::new(),
                definition,
                command.actor_principal_id,
                Utc::now(),
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let profile = match ConnectorProfile::create(profile_id, name, &revision) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let event =
                ConnectorRevisionPublished::created(&profile, &revision, command.request_id)
                    .map_err(|error| BootError::Internal(error.to_string()))?;
            let record = ConnectorRecord::new(profile, revision)
                .map_err(|error| BootError::Internal(error.to_string()))?;
            match connectors
                .create(CreateConnectorProfileWrite {
                    record,
                    event,
                    actor_principal_id: command.actor_principal_id,
                    request_id: command.request_id,
                    idempotency,
                })
                .await
            {
                Ok(result) => Ok(Ok(ConnectorProfileMutationResult {
                    record: result.value,
                    replayed: result.replayed,
                })),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

#[derive(Debug, Clone)]
pub struct ReviseConnectorProfile {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub profile_id: ConnectorProfileId,
    pub expected_version: u64,
    pub definition_acl: String,
    pub actor_principal_id: PrincipalId,
    pub resource_access: ResourceAccessEvaluator,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for ReviseConnectorProfile {
    type Output = ApplicationResult<ConnectorProfileMutationResult>;
}

pub struct ReviseConnectorProfileHandler {
    connectors: Arc<dyn IConnectorProfileRepository>,
    secret_access: ExactSecretVersionAccess,
}

impl ReviseConnectorProfileHandler {
    pub fn new(
        connectors: Arc<dyn IConnectorProfileRepository>,
        secrets: Arc<dyn ISecretRepository>,
    ) -> Self {
        Self {
            connectors,
            secret_access: ExactSecretVersionAccess::new(secrets),
        }
    }
}

impl CommandHandler<ReviseConnectorProfile> for ReviseConnectorProfileHandler {
    fn execute(
        &self,
        command: ReviseConnectorProfile,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<ConnectorProfileMutationResult>>,
    > {
        let connectors = Arc::clone(&self.connectors);
        let secret_access = self.secret_access.clone();
        Box::pin(async move {
            if let Err(error) = environment(
                command.project_id,
                command.environment_id,
                &command.resource_access,
            ) {
                return Ok(Err(error));
            }
            if command.expected_version == 0 {
                return Ok(Err(ApplicationError::Invalid(
                    "expected Connector profile version must be positive".into(),
                )));
            }
            let definition = match ConnectorDefinition::parse_acl(&command.definition_acl) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let canonical = serde_json::to_vec(&CanonicalReviseConnectorProfile {
                organization_id: command.organization_id,
                project_id: command.project_id,
                environment_id: command.environment_id,
                profile_id: command.profile_id,
                expected_version: command.expected_version,
                definition_digest: definition.digest().as_str(),
            })
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/projects/{}/environments/{}/connector-profiles/{}/revisions",
                    command.organization_id,
                    command.project_id,
                    command.environment_id,
                    command.profile_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            match connectors.replay_write(&idempotency).await {
                Ok(Some(record)) => {
                    if !revise_replay_matches(
                        &record,
                        command.organization_id,
                        command.project_id,
                        command.environment_id,
                        command.profile_id,
                        command.expected_version,
                        &definition,
                    ) {
                        return Err(BootError::Internal(
                            "Connector revision replay reference is inconsistent".into(),
                        ));
                    }
                    return Ok(Ok(ConnectorProfileMutationResult {
                        record,
                        replayed: true,
                    }));
                }
                Ok(None) => {}
                Err(error) => return Ok(Err(error.into())),
            }
            let current = match connectors
                .find(
                    command.organization_id,
                    command.project_id,
                    command.environment_id,
                    command.profile_id,
                )
                .await
            {
                Ok(Some(value)) => value,
                Ok(None)
                | Err(crate::modules::shared_kernel::domain::RepositoryError::NotFound) => {
                    return Ok(Err(profile_not_found()))
                }
                Err(error) => return Ok(Err(error.into())),
            };
            if current.aggregate_version != command.expected_version {
                return Ok(Err(ApplicationError::Conflict(
                    "Connector profile was revised from a stale aggregate version".into(),
                )));
            }
            let current_revision = match connectors
                .find_revision(
                    command.organization_id,
                    command.project_id,
                    command.environment_id,
                    command.profile_id,
                    current.current_revision_id,
                )
                .await
            {
                Ok(Some(value)) => value,
                Ok(None)
                | Err(crate::modules::shared_kernel::domain::RepositoryError::NotFound) => {
                    return Err(BootError::Internal(
                        "Connector profile current revision is missing".into(),
                    ))
                }
                Err(error) => return Ok(Err(error.into())),
            };
            if let Err(error) = validate_definition_secret_references(
                &secret_access,
                command.organization_id,
                command.project_id,
                command.environment_id,
                &definition,
            )
            .await
            {
                return Ok(Err(error));
            }
            let revision = match ConnectorRevision::successor(
                &current_revision,
                ConnectorRevisionId::new(),
                definition,
                command.actor_principal_id,
                Utc::now(),
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let profile = match current.advance(command.expected_version, &revision) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Conflict(error))),
            };
            let event =
                ConnectorRevisionPublished::revised(&profile, &revision, command.request_id)
                    .map_err(|error| BootError::Internal(error.to_string()))?;
            let record = ConnectorRecord::new(profile, revision)
                .map_err(|error| BootError::Internal(error.to_string()))?;
            match connectors
                .revise(ReviseConnectorProfileWrite {
                    record,
                    expected_version: command.expected_version,
                    event,
                    actor_principal_id: command.actor_principal_id,
                    request_id: command.request_id,
                    idempotency,
                })
                .await
            {
                Ok(result) => Ok(Ok(ConnectorProfileMutationResult {
                    record: result.value,
                    replayed: result.replayed,
                })),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

fn create_replay_matches(
    record: &ConnectorRecord,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    name: &ResourceName,
    definition: &ConnectorDefinition,
) -> bool {
    record.profile.organization_id == organization_id
        && record.profile.project_id == project_id
        && record.profile.environment_id == environment_id
        && &record.profile.name == name
        && record.revision.definition.digest() == definition.digest()
}

fn revise_replay_matches(
    record: &ConnectorRecord,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    profile_id: ConnectorProfileId,
    expected_version: u64,
    definition: &ConnectorDefinition,
) -> bool {
    record.profile.organization_id == organization_id
        && record.profile.project_id == project_id
        && record.profile.environment_id == environment_id
        && record.profile.id == profile_id
        && record.profile.aggregate_version == expected_version.saturating_add(1)
        && record.revision.definition.digest() == definition.digest()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CanonicalCreateConnectorProfile<'a> {
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    name: &'a str,
    definition_digest: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CanonicalReviseConnectorProfile<'a> {
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    profile_id: ConnectorProfileId,
    expected_version: u64,
    definition_digest: &'a str,
}
