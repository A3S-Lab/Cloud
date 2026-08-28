use super::OutboxNotificationProjector;
use crate::modules::fleet::domain::events::{
    NodeAvailabilityChanged, NodeAvailabilityFactStatus, NodeAvailabilityResolutionReason,
};
use crate::modules::integration_events::OutboxMessage;
use crate::modules::notifications::domain::{
    Notification, NotificationAlertPolicyTarget, NotificationAlertSource, NotificationScope,
    NotificationSeverity,
};
use crate::modules::shared_kernel::domain::RepositoryError;
use a3s_cloud_contracts::DomainEventEnvelope;

impl OutboxNotificationProjector {
    pub(super) async fn node_availability_notifications(
        &self,
        message: &OutboxMessage,
    ) -> Result<Vec<Notification>, RepositoryError> {
        let payload = decode_node_availability(message)?;
        let source = NotificationAlertSource::FleetNodeAvailabilityStatusV1;
        let target = NotificationAlertPolicyTarget::Node {
            node_id: payload.node_id,
        };
        let policies = self
            .authorized_alert_policies(message, source, target)
            .await?;
        let scope = NotificationScope::Node {
            node_id: payload.node_id,
        };
        let mut notifications = Vec::with_capacity(policies.len());
        for policy in policies {
            let (severity, title, body) = match payload.status {
                NodeAvailabilityFactStatus::Unavailable => (
                    NotificationSeverity::Critical,
                    "Node unavailable".to_owned(),
                    format!(
                        "Node {} stopped reporting heartbeats after {}; its last observation was at {}.",
                        payload.node_id,
                        payload.timeout_deadline_at.to_rfc3339(),
                        payload.last_observed_at.to_rfc3339(),
                    ),
                ),
                NodeAvailabilityFactStatus::Resolved => {
                    if !self
                        .recovery_follows_projected_firing(
                            &policy,
                            source,
                            message,
                            "fleet.node.unavailable",
                        )
                        .await?
                    {
                        continue;
                    }
                    let reason = payload.resolution_reason.ok_or_else(|| {
                        RepositoryError::Storage(
                            "notification Node availability resolution reason is missing".into(),
                        )
                    })?;
                    let resolved_at = payload.resolved_at.ok_or_else(|| {
                        RepositoryError::Storage(
                            "notification Node availability resolution time is missing".into(),
                        )
                    })?;
                    let body = match reason {
                        NodeAvailabilityResolutionReason::HeartbeatRestored => format!(
                            "Node {} resumed heartbeats at {}; availability was restored at {}.",
                            payload.node_id,
                            payload.last_observed_at.to_rfc3339(),
                            resolved_at.to_rfc3339(),
                        ),
                        NodeAvailabilityResolutionReason::NodeRevoked => format!(
                            "Node {} was revoked at {}; its unavailable alert is resolved.",
                            payload.node_id,
                            resolved_at.to_rfc3339(),
                        ),
                    };
                    (
                        NotificationSeverity::Information,
                        "Node availability resolved".to_owned(),
                        body,
                    )
                }
            };
            notifications.push(
                Notification::project(
                    policy.organization_id,
                    policy.recipient_principal_id,
                    message.event_id,
                    message.event_key.clone(),
                    message.schema_version,
                    message.aggregate_id,
                    message.aggregate_version,
                    message.correlation_id,
                    severity,
                    title,
                    body,
                    scope,
                    message.occurred_at,
                    message.occurred_at,
                )
                .map_err(RepositoryError::Storage)?,
            );
        }
        Ok(notifications)
    }
}

pub(super) fn decode_node_availability(
    message: &OutboxMessage,
) -> Result<NodeAvailabilityChanged, RepositoryError> {
    NodeAvailabilityChanged::decode_envelope(&DomainEventEnvelope {
        event_id: message.event_id,
        event_key: message.event_key.clone(),
        schema_version: message.schema_version,
        scope: message.scope.reference(),
        aggregate_id: message.aggregate_id,
        aggregate_version: message.aggregate_version,
        occurred_at: message.occurred_at,
        correlation_id: message.correlation_id,
        causation_id: message.causation_id,
        payload: message.payload.clone(),
    })
    .map_err(|error| {
        RepositoryError::Storage(format!(
            "notification source Node availability event is invalid: {error}"
        ))
    })
}
