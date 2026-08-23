use super::OutboxNotificationProjector;
use crate::modules::edge::domain::events::{
    GatewayCertificateExpiryChanged, GatewayCertificateExpiryStatus,
};
use crate::modules::integration_events::OutboxMessage;
use crate::modules::notifications::domain::{
    Notification, NotificationAlertPolicyTarget, NotificationAlertSource, NotificationScope,
    NotificationSeverity,
};
use crate::modules::shared_kernel::domain::RepositoryError;
use a3s_cloud_contracts::DomainEventEnvelope;

impl OutboxNotificationProjector {
    pub(super) async fn gateway_certificate_expiry_notifications(
        &self,
        message: &OutboxMessage,
    ) -> Result<Vec<Notification>, RepositoryError> {
        let payload = decode_gateway_certificate_expiry(message)?;
        let source = NotificationAlertSource::EdgeGatewayCertificateExpiryStatusV1;
        let policies = self
            .authorized_alert_policies(
                message,
                source,
                NotificationAlertPolicyTarget::Environment {
                    project_id: payload.project_id,
                    environment_id: payload.environment_id,
                },
            )
            .await?;
        let scope = NotificationScope::Environment {
            project_id: payload.project_id,
            environment_id: payload.environment_id,
        };
        let expires_at = payload.active_certificate_expires_at.to_rfc3339();
        let mut notifications = Vec::with_capacity(policies.len());
        for policy in policies {
            let (severity, title, body) = match payload.status {
                GatewayCertificateExpiryStatus::Expiring => (
                    NotificationSeverity::Warning,
                    "Gateway certificate expiring".to_owned(),
                    format!(
                        "The active certificate for {} (Route {}) is expiring on Gateway node {} at {}; a replacement is staged.",
                        payload.hostname, payload.route_id, payload.node_id, expires_at
                    ),
                ),
                GatewayCertificateExpiryStatus::Resolved => {
                    if !self
                        .recovery_follows_projected_firing(
                            &policy,
                            source,
                            message,
                            "edge.gateway-certificate.expiring",
                        )
                        .await?
                    {
                        continue;
                    }
                    (
                        NotificationSeverity::Information,
                        "Gateway certificate expiry resolved".to_owned(),
                        format!(
                            "The replacement certificate for {} (Route {}) is active on Gateway node {} and expires at {}.",
                            payload.hostname, payload.route_id, payload.node_id, expires_at
                        ),
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

pub(super) fn decode_gateway_certificate_expiry(
    message: &OutboxMessage,
) -> Result<GatewayCertificateExpiryChanged, RepositoryError> {
    GatewayCertificateExpiryChanged::decode_envelope(&DomainEventEnvelope {
        event_id: message.event_id,
        event_key: message.event_key.clone(),
        schema_version: message.schema_version,
        organization_id: message.organization_id,
        aggregate_id: message.aggregate_id,
        aggregate_version: message.aggregate_version,
        occurred_at: message.occurred_at,
        correlation_id: message.correlation_id,
        causation_id: message.causation_id,
        payload: message.payload.clone(),
    })
    .map_err(|error| {
        RepositoryError::Storage(format!(
            "notification source Gateway certificate expiry event is invalid: {error}"
        ))
    })
}
