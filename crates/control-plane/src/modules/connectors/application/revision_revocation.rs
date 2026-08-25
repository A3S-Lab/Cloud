use super::resource_access::{environment, revision_not_found, revision_revocation_not_found};
use crate::modules::connectors::domain::{
    normalize_connector_revision_revocation_reason, ConnectorRevisionRevocation,
    ConnectorRevisionRevoked, IConnectorProfileRepository, IConnectorRevisionRevocationRepository,
    RevokeConnectorRevisionWrite,
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
pub struct RevokeConnectorRevision {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub profile_id: ConnectorProfileId,
    pub revision_id: ConnectorRevisionId,
    pub reason: String,
    pub actor_principal_id: PrincipalId,
    pub resource_access: ResourceAccessEvaluator,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for RevokeConnectorRevision {
    type Output = ApplicationResult<ConnectorRevisionRevocationMutationResult>;
}

pub struct RevokeConnectorRevisionHandler {
    profiles: Arc<dyn IConnectorProfileRepository>,
    revocations: Arc<dyn IConnectorRevisionRevocationRepository>,
}

impl RevokeConnectorRevisionHandler {
    pub fn new(
        profiles: Arc<dyn IConnectorProfileRepository>,
        revocations: Arc<dyn IConnectorRevisionRevocationRepository>,
    ) -> Self {
        Self {
            profiles,
            revocations,
        }
    }
}

impl CommandHandler<RevokeConnectorRevision> for RevokeConnectorRevisionHandler {
    fn execute(
        &self,
        command: RevokeConnectorRevision,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<ConnectorRevisionRevocationMutationResult>>,
    > {
        let profiles = Arc::clone(&self.profiles);
        let revocations = Arc::clone(&self.revocations);
        Box::pin(async move {
            if let Err(error) = environment(
                command.project_id,
                command.environment_id,
                &command.resource_access,
            ) {
                return Ok(Err(error));
            }
            let reason =
                match normalize_connector_revision_revocation_reason(command.reason.as_str()) {
                    Ok(reason) => reason,
                    Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
                };
            let canonical = serde_json::to_vec(&CanonicalRevokeConnectorRevision {
                organization_id: command.organization_id,
                project_id: command.project_id,
                environment_id: command.environment_id,
                profile_id: command.profile_id,
                revision_id: command.revision_id,
                reason: &reason,
            })
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "connector-revision-revocation/{}/{}",
                    command.organization_id, command.revision_id,
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(idempotency) => idempotency,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            match revocations.replay_revocation_write(&idempotency).await {
                Ok(Some(revocation)) => {
                    if !matches_command(
                        &revocation,
                        command.organization_id,
                        command.project_id,
                        command.environment_id,
                        command.profile_id,
                        command.revision_id,
                        &reason,
                    ) {
                        return Err(BootError::Internal(
                            "Connector revocation replay authority is inconsistent".into(),
                        ));
                    }
                    return Ok(Ok(ConnectorRevisionRevocationMutationResult {
                        revocation,
                        replayed: true,
                    }));
                }
                Ok(None) => {}
                Err(error) => return Ok(Err(error.into())),
            }
            let revision = match profiles
                .find_revision(
                    command.organization_id,
                    command.project_id,
                    command.environment_id,
                    command.profile_id,
                    command.revision_id,
                )
                .await
            {
                Ok(Some(revision)) => revision,
                Ok(None) | Err(RepositoryError::NotFound) => return Ok(Err(revision_not_found())),
                Err(error) => return Ok(Err(error.into())),
            };
            let revocation = match ConnectorRevisionRevocation::new(
                &revision,
                reason,
                command.actor_principal_id,
                Utc::now().max(revision.created_at),
            ) {
                Ok(revocation) => revocation,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let event = ConnectorRevisionRevoked::envelope(&revocation, command.request_id)
                .map_err(|error| BootError::Internal(error.to_string()))?;
            match revocations
                .revoke_revision(RevokeConnectorRevisionWrite {
                    revocation,
                    event,
                    actor_principal_id: command.actor_principal_id,
                    request_id: command.request_id,
                    idempotency,
                })
                .await
            {
                Ok(write) => Ok(Ok(ConnectorRevisionRevocationMutationResult {
                    revocation: write.value,
                    replayed: write.replayed,
                })),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

#[derive(Debug, Clone)]
pub struct GetConnectorRevisionRevocation {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub profile_id: ConnectorProfileId,
    pub revision_id: ConnectorRevisionId,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for GetConnectorRevisionRevocation {
    type Output = ApplicationResult<ConnectorRevisionRevocation>;
}

pub struct GetConnectorRevisionRevocationHandler {
    revocations: Arc<dyn IConnectorRevisionRevocationRepository>,
}

impl GetConnectorRevisionRevocationHandler {
    pub fn new(revocations: Arc<dyn IConnectorRevisionRevocationRepository>) -> Self {
        Self { revocations }
    }
}

impl QueryHandler<GetConnectorRevisionRevocation> for GetConnectorRevisionRevocationHandler {
    fn execute(
        &self,
        query: GetConnectorRevisionRevocation,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<ConnectorRevisionRevocation>>,
    > {
        let revocations = Arc::clone(&self.revocations);
        Box::pin(async move {
            if let Err(error) = environment(
                query.project_id,
                query.environment_id,
                &query.resource_access,
            ) {
                return Ok(Err(error));
            }
            Ok(
                match revocations
                    .find_revision_revocation(
                        query.organization_id,
                        query.project_id,
                        query.environment_id,
                        query.profile_id,
                        query.revision_id,
                    )
                    .await
                {
                    Ok(Some(revocation)) => Ok(revocation),
                    Ok(None) | Err(RepositoryError::NotFound) => {
                        Err(revision_revocation_not_found())
                    }
                    Err(error) => Err(error.into()),
                },
            )
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorRevisionRevocationMutationResult {
    pub revocation: ConnectorRevisionRevocation,
    pub replayed: bool,
}

fn matches_command(
    revocation: &ConnectorRevisionRevocation,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    profile_id: ConnectorProfileId,
    revision_id: ConnectorRevisionId,
    reason: &str,
) -> bool {
    revocation.organization_id == organization_id
        && revocation.project_id == project_id
        && revocation.environment_id == environment_id
        && revocation.profile_id == profile_id
        && revocation.revision_id == revision_id
        && revocation.reason == reason
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CanonicalRevokeConnectorRevision<'a> {
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    profile_id: ConnectorProfileId,
    revision_id: ConnectorRevisionId,
    reason: &'a str,
}
