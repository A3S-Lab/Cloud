use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::identity::domain::value_objects::ResourceGrantScope;
use crate::modules::notifications::domain::{
    CreateNotificationAlertPolicyWrite, INotificationAlertPolicyRepository,
    NotificationAlertPolicy, NotificationAlertPolicyDefinition, NotificationAlertPolicyEvent,
    RevokeNotificationAlertPolicyWrite,
};
use crate::modules::projects::domain::repositories::IEnvironmentRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    IdempotencyRequest, NotificationAlertPolicyId, OrganizationId, PrincipalId,
};
use a3s_boot::{BootError, Command, CommandHandler, CqrsContext};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CreateNotificationAlertPolicy {
    pub organization_id: OrganizationId,
    pub definition_acl: String,
    pub actor_principal_id: PrincipalId,
    pub resource_access: ResourceAccessEvaluator,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for CreateNotificationAlertPolicy {
    type Output = ApplicationResult<NotificationAlertPolicyMutationResult>;
}

#[derive(Debug, Clone)]
pub struct RevokeNotificationAlertPolicy {
    pub organization_id: OrganizationId,
    pub policy_id: NotificationAlertPolicyId,
    pub expected_version: u64,
    pub actor_principal_id: PrincipalId,
    pub resource_access: ResourceAccessEvaluator,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for RevokeNotificationAlertPolicy {
    type Output = ApplicationResult<NotificationAlertPolicyMutationResult>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationAlertPolicyMutationResult {
    pub policy: NotificationAlertPolicy,
    pub replayed: bool,
}

pub struct CreateNotificationAlertPolicyHandler {
    notifications: Arc<dyn INotificationAlertPolicyRepository>,
    environments: Arc<dyn IEnvironmentRepository>,
}

impl CreateNotificationAlertPolicyHandler {
    pub fn new(
        notifications: Arc<dyn INotificationAlertPolicyRepository>,
        environments: Arc<dyn IEnvironmentRepository>,
    ) -> Self {
        Self {
            notifications,
            environments,
        }
    }
}

impl CommandHandler<CreateNotificationAlertPolicy> for CreateNotificationAlertPolicyHandler {
    fn execute(
        &self,
        command: CreateNotificationAlertPolicy,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<NotificationAlertPolicyMutationResult>>,
    > {
        let notifications = Arc::clone(&self.notifications);
        let environments = Arc::clone(&self.environments);
        Box::pin(async move {
            if command.organization_id.as_uuid().is_nil()
                || command.actor_principal_id.as_uuid().is_nil()
                || command.request_id.is_nil()
            {
                return Ok(Err(ApplicationError::Invalid(
                    "notification alert policy actor or request is invalid".into(),
                )));
            }
            let definition =
                match NotificationAlertPolicyDefinition::parse_acl(&command.definition_acl) {
                    Ok(value) => value,
                    Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
                };
            let spec = definition.spec();
            if !command
                .resource_access
                .allows(ResourceGrantScope::Environment {
                    project_id: spec.project_id,
                    environment_id: spec.environment_id,
                })
            {
                return Ok(Err(alert_policy_not_found()));
            }
            match environments
                .find(
                    command.organization_id,
                    spec.project_id,
                    spec.environment_id,
                )
                .await
            {
                Ok(Some(environment))
                    if environment.organization_id == command.organization_id
                        && environment.project_id == spec.project_id
                        && environment.id == spec.environment_id => {}
                Ok(Some(_)) => {
                    return Err(BootError::Internal(
                        "environment lookup returned inconsistent identity".into(),
                    ))
                }
                Ok(None)
                | Err(crate::modules::shared_kernel::domain::RepositoryError::NotFound) => {
                    return Ok(Err(alert_policy_not_found()))
                }
                Err(error) => return Ok(Err(error.into())),
            }
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organizationId": command.organization_id,
                "recipientPrincipalId": command.actor_principal_id,
                "definitionDigest": definition.digest(),
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/principals/{}/notification-alert-policies",
                    command.organization_id, command.actor_principal_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            match notifications.replay_alert_policy_write(&idempotency).await {
                Ok(Some(replayed)) => {
                    if replayed.value.organization_id != command.organization_id
                        || replayed.value.recipient_principal_id != command.actor_principal_id
                        || replayed.value.definition != definition
                    {
                        return Err(BootError::Internal(
                            "notification alert policy create replay changed identity".into(),
                        ));
                    }
                    return Ok(Ok(NotificationAlertPolicyMutationResult {
                        policy: replayed.value,
                        replayed: true,
                    }));
                }
                Ok(None) => {}
                Err(error) => return Ok(Err(error.into())),
            }
            let policy = match NotificationAlertPolicy::create(
                command.organization_id,
                NotificationAlertPolicyId::new(),
                command.actor_principal_id,
                definition,
                command.actor_principal_id,
                Utc::now(),
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let event = NotificationAlertPolicyEvent::envelope(
                "notification.alert-policy.created",
                &policy,
                command.request_id,
            )
            .map_err(BootError::Internal)?;
            match notifications
                .create_alert_policy(CreateNotificationAlertPolicyWrite {
                    policy,
                    event,
                    actor_principal_id: command.actor_principal_id,
                    request_id: command.request_id,
                    idempotency,
                })
                .await
            {
                Ok(result) => Ok(Ok(NotificationAlertPolicyMutationResult {
                    policy: result.value,
                    replayed: result.replayed,
                })),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

pub struct RevokeNotificationAlertPolicyHandler {
    notifications: Arc<dyn INotificationAlertPolicyRepository>,
}

impl RevokeNotificationAlertPolicyHandler {
    pub fn new(notifications: Arc<dyn INotificationAlertPolicyRepository>) -> Self {
        Self { notifications }
    }
}

impl CommandHandler<RevokeNotificationAlertPolicy> for RevokeNotificationAlertPolicyHandler {
    fn execute(
        &self,
        command: RevokeNotificationAlertPolicy,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<NotificationAlertPolicyMutationResult>>,
    > {
        let notifications = Arc::clone(&self.notifications);
        Box::pin(async move {
            if command.organization_id.as_uuid().is_nil()
                || command.policy_id.as_uuid().is_nil()
                || command.actor_principal_id.as_uuid().is_nil()
                || command.request_id.is_nil()
                || command.expected_version == 0
            {
                return Ok(Err(ApplicationError::Invalid(
                    "notification alert policy revoke identity or version is invalid".into(),
                )));
            }
            let existing = match notifications
                .find_alert_policy(
                    command.organization_id,
                    command.actor_principal_id,
                    command.policy_id,
                )
                .await
            {
                Ok(Some(value)) => value,
                Ok(None)
                | Err(crate::modules::shared_kernel::domain::RepositoryError::NotFound) => {
                    return Ok(Err(alert_policy_not_found()))
                }
                Err(error) => return Ok(Err(error.into())),
            };
            let spec = existing.definition.spec();
            if !command
                .resource_access
                .allows(ResourceGrantScope::Environment {
                    project_id: spec.project_id,
                    environment_id: spec.environment_id,
                })
            {
                return Ok(Err(alert_policy_not_found()));
            }
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organizationId": command.organization_id,
                "policyId": command.policy_id,
                "expectedVersion": command.expected_version,
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/principals/{}/notification-alert-policies/{}/revoke",
                    command.organization_id, command.actor_principal_id, command.policy_id,
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            match notifications.replay_alert_policy_write(&idempotency).await {
                Ok(Some(replayed)) => {
                    if replayed.value.organization_id != command.organization_id
                        || replayed.value.id != command.policy_id
                        || replayed.value.recipient_principal_id != command.actor_principal_id
                        || replayed.value.aggregate_version != command.expected_version + 1
                        || replayed.value.is_active()
                    {
                        return Err(BootError::Internal(
                            "notification alert policy revoke replay changed identity".into(),
                        ));
                    }
                    return Ok(Ok(NotificationAlertPolicyMutationResult {
                        policy: replayed.value,
                        replayed: true,
                    }));
                }
                Ok(None) => {}
                Err(error) => return Ok(Err(error.into())),
            }
            let policy = match existing.revoke(
                command.expected_version,
                command.actor_principal_id,
                Utc::now(),
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Conflict(error))),
            };
            let event = NotificationAlertPolicyEvent::envelope(
                "notification.alert-policy.revoked",
                &policy,
                command.request_id,
            )
            .map_err(BootError::Internal)?;
            match notifications
                .revoke_alert_policy(RevokeNotificationAlertPolicyWrite {
                    policy,
                    expected_version: command.expected_version,
                    event,
                    actor_principal_id: command.actor_principal_id,
                    request_id: command.request_id,
                    idempotency,
                })
                .await
            {
                Ok(result) => Ok(Ok(NotificationAlertPolicyMutationResult {
                    policy: result.value,
                    replayed: result.replayed,
                })),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

pub(super) fn alert_policy_not_found() -> ApplicationError {
    ApplicationError::NotFound("notification alert policy not found".into())
}

#[cfg(test)]
#[path = "alert_policy_tests.rs"]
mod tests;
