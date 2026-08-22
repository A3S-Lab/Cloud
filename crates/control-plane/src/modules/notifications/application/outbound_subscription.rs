use crate::modules::connectors::IConnectorProfileRepository;
use crate::modules::identity::domain::repositories::IRecipientContactRepository;
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::identity::domain::value_objects::ResourceGrantScope;
use crate::modules::notifications::domain::{
    CreateOutboundNotificationSubscriptionWrite, IOutboundNotificationRepository,
    OutboundNotificationSubscription, OutboundNotificationSubscriptionDefinition,
    OutboundNotificationSubscriptionEvent, RevokeOutboundNotificationSubscriptionWrite,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    IdempotencyRequest, NotificationSubscriptionId, OrganizationId, PrincipalId,
};
use a3s_boot::{BootError, Command, CommandHandler, CqrsContext};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CreateOutboundNotificationSubscription {
    pub organization_id: OrganizationId,
    pub definition_acl: String,
    pub actor_principal_id: PrincipalId,
    pub resource_access: ResourceAccessEvaluator,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for CreateOutboundNotificationSubscription {
    type Output = ApplicationResult<OutboundNotificationSubscriptionMutationResult>;
}

#[derive(Debug, Clone)]
pub struct RevokeOutboundNotificationSubscription {
    pub organization_id: OrganizationId,
    pub subscription_id: NotificationSubscriptionId,
    pub expected_version: u64,
    pub actor_principal_id: PrincipalId,
    pub resource_access: ResourceAccessEvaluator,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for RevokeOutboundNotificationSubscription {
    type Output = ApplicationResult<OutboundNotificationSubscriptionMutationResult>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutboundNotificationSubscriptionMutationResult {
    pub subscription: OutboundNotificationSubscription,
    pub replayed: bool,
}

pub struct CreateOutboundNotificationSubscriptionHandler {
    notifications: Arc<dyn IOutboundNotificationRepository>,
    connectors: Arc<dyn IConnectorProfileRepository>,
    recipient_contacts: Arc<dyn IRecipientContactRepository>,
}

impl CreateOutboundNotificationSubscriptionHandler {
    pub fn new(
        notifications: Arc<dyn IOutboundNotificationRepository>,
        connectors: Arc<dyn IConnectorProfileRepository>,
        recipient_contacts: Arc<dyn IRecipientContactRepository>,
    ) -> Self {
        Self {
            notifications,
            connectors,
            recipient_contacts,
        }
    }
}

impl CommandHandler<CreateOutboundNotificationSubscription>
    for CreateOutboundNotificationSubscriptionHandler
{
    fn execute(
        &self,
        command: CreateOutboundNotificationSubscription,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<OutboundNotificationSubscriptionMutationResult>>,
    > {
        let notifications = Arc::clone(&self.notifications);
        let connectors = Arc::clone(&self.connectors);
        let recipient_contacts = Arc::clone(&self.recipient_contacts);
        Box::pin(async move {
            if command.organization_id.as_uuid().is_nil()
                || command.actor_principal_id.as_uuid().is_nil()
                || command.request_id.is_nil()
            {
                return Ok(Err(ApplicationError::Invalid(
                    "outbound notification subscription actor or request is invalid".into(),
                )));
            }
            let definition = match OutboundNotificationSubscriptionDefinition::parse_acl(
                &command.definition_acl,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let spec = definition.spec();
            if let Some(target) = spec.target.connector() {
                if !command
                    .resource_access
                    .allows(ResourceGrantScope::Environment {
                        project_id: target.project_id,
                        environment_id: target.environment_id,
                    })
                {
                    return Ok(Err(outbound_subscription_not_found()));
                }
            } else if let Some(contact_id) = spec.target.recipient_contact_id() {
                match recipient_contacts
                    .resolve_verified_recipient_contact(
                        command.organization_id,
                        command.actor_principal_id,
                        contact_id,
                    )
                    .await
                {
                    Ok(Some(contact))
                        if contact.id == contact_id
                            && contact.principal_id == command.actor_principal_id => {}
                    Ok(Some(_)) => {
                        return Err(BootError::Internal(
                            "recipient contact lookup returned inconsistent identity".into(),
                        ))
                    }
                    Ok(None)
                    | Err(crate::modules::shared_kernel::domain::RepositoryError::NotFound) => {
                        return Ok(Err(outbound_subscription_not_found()))
                    }
                    Err(error) => return Ok(Err(error.into())),
                }
            }
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organizationId": command.organization_id,
                "recipientPrincipalId": command.actor_principal_id,
                "definitionDigest": definition.digest(),
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/principals/{}/notification-outbound-subscriptions",
                    command.organization_id, command.actor_principal_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            match notifications.replay_subscription_write(&idempotency).await {
                Ok(Some(replayed)) => {
                    if replayed.value.organization_id != command.organization_id
                        || replayed.value.recipient_principal_id != command.actor_principal_id
                        || replayed.value.definition != definition
                    {
                        return Err(BootError::Internal(
                            "outbound notification subscription create replay changed identity"
                                .into(),
                        ));
                    }
                    return Ok(Ok(OutboundNotificationSubscriptionMutationResult {
                        subscription: replayed.value,
                        replayed: true,
                    }));
                }
                Ok(None) => {}
                Err(error) => return Ok(Err(error.into())),
            }
            if let Some(target) = spec.target.connector() {
                match connectors
                    .find_revision(
                        command.organization_id,
                        target.project_id,
                        target.environment_id,
                        target.profile_id,
                        target.revision_id,
                    )
                    .await
                {
                    Ok(Some(revision))
                        if revision.organization_id == command.organization_id
                            && revision.project_id == target.project_id
                            && revision.environment_id == target.environment_id
                            && revision.profile_id == target.profile_id
                            && revision.id == target.revision_id => {}
                    Ok(Some(_)) => {
                        return Err(BootError::Internal(
                            "Connector revision lookup returned inconsistent identity".into(),
                        ))
                    }
                    Ok(None)
                    | Err(crate::modules::shared_kernel::domain::RepositoryError::NotFound) => {
                        return Ok(Err(outbound_subscription_not_found()))
                    }
                    Err(error) => return Ok(Err(error.into())),
                }
            }
            let subscription = match OutboundNotificationSubscription::create(
                command.organization_id,
                NotificationSubscriptionId::new(),
                command.actor_principal_id,
                definition,
                command.actor_principal_id,
                Utc::now(),
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let event = OutboundNotificationSubscriptionEvent::envelope(
                "notification.outbound-subscription.created",
                &subscription,
                command.request_id,
            )
            .map_err(BootError::Internal)?;
            match notifications
                .create_subscription(CreateOutboundNotificationSubscriptionWrite {
                    subscription,
                    event,
                    actor_principal_id: command.actor_principal_id,
                    request_id: command.request_id,
                    idempotency,
                })
                .await
            {
                Ok(result) => Ok(Ok(OutboundNotificationSubscriptionMutationResult {
                    subscription: result.value,
                    replayed: result.replayed,
                })),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

pub struct RevokeOutboundNotificationSubscriptionHandler {
    notifications: Arc<dyn IOutboundNotificationRepository>,
}

impl RevokeOutboundNotificationSubscriptionHandler {
    pub fn new(notifications: Arc<dyn IOutboundNotificationRepository>) -> Self {
        Self { notifications }
    }
}

impl CommandHandler<RevokeOutboundNotificationSubscription>
    for RevokeOutboundNotificationSubscriptionHandler
{
    fn execute(
        &self,
        command: RevokeOutboundNotificationSubscription,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<OutboundNotificationSubscriptionMutationResult>>,
    > {
        let notifications = Arc::clone(&self.notifications);
        Box::pin(async move {
            if command.organization_id.as_uuid().is_nil()
                || command.subscription_id.as_uuid().is_nil()
                || command.actor_principal_id.as_uuid().is_nil()
                || command.request_id.is_nil()
                || command.expected_version == 0
            {
                return Ok(Err(ApplicationError::Invalid(
                    "outbound notification subscription revoke identity or version is invalid"
                        .into(),
                )));
            }
            let existing = match notifications
                .find_subscription(
                    command.organization_id,
                    command.actor_principal_id,
                    command.subscription_id,
                )
                .await
            {
                Ok(Some(value)) => value,
                Ok(None)
                | Err(crate::modules::shared_kernel::domain::RepositoryError::NotFound) => {
                    return Ok(Err(outbound_subscription_not_found()))
                }
                Err(error) => return Ok(Err(error.into())),
            };
            let target = existing.definition.spec().target;
            if let Some(target) = target.connector() {
                if !command
                    .resource_access
                    .allows(ResourceGrantScope::Environment {
                        project_id: target.project_id,
                        environment_id: target.environment_id,
                    })
                {
                    return Ok(Err(outbound_subscription_not_found()));
                }
            }
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organizationId": command.organization_id,
                "subscriptionId": command.subscription_id,
                "expectedVersion": command.expected_version,
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/principals/{}/notification-outbound-subscriptions/{}/revoke",
                    command.organization_id, command.actor_principal_id, command.subscription_id,
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            match notifications.replay_subscription_write(&idempotency).await {
                Ok(Some(replayed)) => {
                    if replayed.value.organization_id != command.organization_id
                        || replayed.value.id != command.subscription_id
                        || replayed.value.recipient_principal_id != command.actor_principal_id
                        || replayed.value.aggregate_version != command.expected_version + 1
                        || replayed.value.is_active()
                    {
                        return Err(BootError::Internal(
                            "outbound notification subscription revoke replay changed identity"
                                .into(),
                        ));
                    }
                    return Ok(Ok(OutboundNotificationSubscriptionMutationResult {
                        subscription: replayed.value,
                        replayed: true,
                    }));
                }
                Ok(None) => {}
                Err(error) => return Ok(Err(error.into())),
            }
            let subscription = match existing.revoke(
                command.expected_version,
                command.actor_principal_id,
                Utc::now(),
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Conflict(error))),
            };
            let event = OutboundNotificationSubscriptionEvent::envelope(
                "notification.outbound-subscription.revoked",
                &subscription,
                command.request_id,
            )
            .map_err(BootError::Internal)?;
            match notifications
                .revoke_subscription(RevokeOutboundNotificationSubscriptionWrite {
                    subscription,
                    expected_version: command.expected_version,
                    event,
                    actor_principal_id: command.actor_principal_id,
                    request_id: command.request_id,
                    idempotency,
                })
                .await
            {
                Ok(result) => Ok(Ok(OutboundNotificationSubscriptionMutationResult {
                    subscription: result.value,
                    replayed: result.replayed,
                })),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

pub(super) fn outbound_subscription_not_found() -> ApplicationError {
    ApplicationError::NotFound("outbound notification subscription not found".into())
}

#[cfg(test)]
#[path = "outbound_subscription_tests.rs"]
mod tests;
