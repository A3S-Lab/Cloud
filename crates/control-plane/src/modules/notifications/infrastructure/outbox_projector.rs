use crate::modules::edge::domain::events::{
    renewal_subject_id, DomainClaimChanged, GatewayCertificateRenewalChanged,
    GatewayCertificateRenewalFailureKind, GatewayCertificateRenewalStatus,
};
use crate::modules::edge::domain::{DomainClaimState, DomainNamePattern, RouteHostname, RoutePath};
use crate::modules::identity::domain::events::MembershipChanged;
use crate::modules::identity::domain::repositories::{
    IMembershipRepository, IResourceGrantRepository,
};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::identity::domain::value_objects::MembershipRole;
use crate::modules::integration_events::{IIntegrationEventProjector, OutboxMessage};
use crate::modules::notifications::domain::{
    INotificationAlertPolicyRepository, INotificationRepository, Notification,
    NotificationAlertPolicy, NotificationAlertSource, NotificationScope, NotificationSeverity,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, OrganizationId, PrincipalId, ProjectId, RepositoryError,
};
use async_trait::async_trait;
use std::sync::Arc;

pub struct OutboxNotificationProjector {
    notifications: Arc<dyn INotificationRepository>,
    memberships: Arc<dyn IMembershipRepository>,
    alert_policies: Option<Arc<dyn INotificationAlertPolicyRepository>>,
    resource_grants: Option<Arc<dyn IResourceGrantRepository>>,
}

impl OutboxNotificationProjector {
    pub fn new(
        notifications: Arc<dyn INotificationRepository>,
        memberships: Arc<dyn IMembershipRepository>,
    ) -> Self {
        Self {
            notifications,
            memberships,
            alert_policies: None,
            resource_grants: None,
        }
    }

    pub fn with_alert_policies(
        mut self,
        alert_policies: Arc<dyn INotificationAlertPolicyRepository>,
        resource_grants: Arc<dyn IResourceGrantRepository>,
    ) -> Self {
        self.alert_policies = Some(alert_policies);
        self.resource_grants = Some(resource_grants);
        self
    }

    async fn notifications_for(
        &self,
        message: &OutboxMessage,
    ) -> Result<Vec<Notification>, RepositoryError> {
        if message.schema_version != 1 {
            return Ok(Vec::new());
        }
        let (recipient, severity, title, body) = match message.event_key.as_str() {
            "identity.membership.created" => {
                let payload = decode_membership(message)?;
                (
                    PrincipalId::from_uuid(payload.principal_id),
                    NotificationSeverity::Information,
                    "Organization access granted".to_owned(),
                    format!("You can now access this organization as {}.", payload.role),
                )
            }
            "identity.membership.role-changed" => {
                let payload = decode_membership(message)?;
                (
                    PrincipalId::from_uuid(payload.principal_id),
                    NotificationSeverity::Information,
                    "Organization role changed".to_owned(),
                    format!("Your organization role is now {}.", payload.role),
                )
            }
            "edge.domain-claim.rejected" | "edge.domain-claim.verified" => {
                return self.domain_claim_notifications(message).await;
            }
            "edge.gateway-certificate.renewal-failed" | "edge.gateway-certificate.renewed" => {
                return self
                    .gateway_certificate_renewal_notifications(message)
                    .await;
            }
            _ => return Ok(Vec::new()),
        };

        // The organization-scoped inbox is reachable only by active members. Invitation and
        // revocation facts therefore remain in their existing lifecycle surfaces instead of
        // creating dead inbox records. A delayed fact is also skipped if access has since ended.
        let membership = self
            .memberships
            .find_membership(
                OrganizationId::from_uuid(message.organization_id),
                crate::modules::shared_kernel::domain::MembershipId::from_uuid(
                    message.aggregate_id,
                ),
            )
            .await?
            .ok_or_else(|| {
                RepositoryError::Storage("notification source membership no longer exists".into())
            })?;
        if membership.membership.principal_id != recipient {
            return Err(RepositoryError::Storage(
                "notification source membership principal is inconsistent".into(),
            ));
        }
        if !membership.membership.is_active() {
            return Ok(Vec::new());
        }

        Notification::project(
            OrganizationId::from_uuid(message.organization_id),
            recipient,
            message.event_id,
            message.event_key.clone(),
            message.schema_version,
            message.aggregate_id,
            message.aggregate_version,
            message.correlation_id,
            severity,
            title,
            body,
            NotificationScope::Organization,
            message.occurred_at,
            // This is the logical in-app delivery time. Deriving it from the immutable source
            // fact makes a relay retry an exact projection replay.
            message.occurred_at,
        )
        .map(|notification| vec![notification])
        .map_err(RepositoryError::Storage)
    }

    async fn authorized_alert_policies(
        &self,
        message: &OutboxMessage,
        source: NotificationAlertSource,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Result<Vec<NotificationAlertPolicy>, RepositoryError> {
        let (Some(alert_policies), Some(resource_grants)) =
            (&self.alert_policies, &self.resource_grants)
        else {
            return Ok(Vec::new());
        };
        let policies = alert_policies
            .list_active_alert_policies_for_source(
                OrganizationId::from_uuid(message.organization_id),
                source,
                project_id,
                environment_id,
                message.occurred_at,
            )
            .await?;
        let scope = NotificationScope::Environment {
            project_id,
            environment_id,
        };
        let mut authorized = Vec::with_capacity(policies.len());
        for policy in policies {
            let Some(membership) = self
                .memberships
                .find_active_membership_by_principal(
                    policy.organization_id,
                    policy.recipient_principal_id,
                )
                .await?
            else {
                continue;
            };
            let grants = resource_grants
                .list_active_resource_grants_for_membership(policy.organization_id, membership.id)
                .await?;
            let access = ResourceAccessEvaluator::for_membership(
                membership.role,
                grants.into_iter().map(|grant| grant.scope),
            );
            if !scope.is_visible_to(&access) {
                continue;
            }
            authorized.push(policy);
        }
        Ok(authorized)
    }

    async fn domain_claim_notifications(
        &self,
        message: &OutboxMessage,
    ) -> Result<Vec<Notification>, RepositoryError> {
        let payload = decode_domain_claim(message)?;
        let source = NotificationAlertSource::EdgeDomainClaimStatusV1;
        let policies = self
            .authorized_alert_policies(message, source, payload.project_id, payload.environment_id)
            .await?;
        let scope = NotificationScope::Environment {
            project_id: payload.project_id,
            environment_id: payload.environment_id,
        };
        let mut notifications = Vec::with_capacity(policies.len());
        for policy in policies {
            let (severity, title, body) = match payload.state {
                DomainClaimState::Rejected => (
                    NotificationSeverity::Warning,
                    "Domain claim rejected".to_owned(),
                    format!(
                        "{} could not be verified. Review its domain ownership challenge.",
                        payload.pattern
                    ),
                ),
                DomainClaimState::Verified => {
                    if !policy.definition.spec().notify_on_recovery {
                        continue;
                    }
                    let latest = self
                        .notifications
                        .latest_alert_source_projection(
                            policy.organization_id,
                            policy.recipient_principal_id,
                            source,
                            message.aggregate_id,
                            policy.created_at,
                            message.aggregate_version,
                        )
                        .await?;
                    if latest
                        .as_ref()
                        .map(|notification| notification.source_event_key.as_str())
                        != Some("edge.domain-claim.rejected")
                    {
                        continue;
                    }
                    (
                        NotificationSeverity::Information,
                        "Domain claim recovered".to_owned(),
                        format!("{} is now verified.", payload.pattern),
                    )
                }
                DomainClaimState::Pending | DomainClaimState::Revoked => {
                    return Err(RepositoryError::Storage(
                        "notification domain claim source state is unsupported".into(),
                    ));
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

    async fn gateway_certificate_renewal_notifications(
        &self,
        message: &OutboxMessage,
    ) -> Result<Vec<Notification>, RepositoryError> {
        let payload = decode_gateway_certificate_renewal(message)?;
        let source = NotificationAlertSource::EdgeGatewayCertificateRenewalStatusV1;
        let policies = self
            .authorized_alert_policies(message, source, payload.project_id, payload.environment_id)
            .await?;
        let scope = NotificationScope::Environment {
            project_id: payload.project_id,
            environment_id: payload.environment_id,
        };
        let expires_at = payload.active_certificate_expires_at.to_rfc3339();
        let mut notifications = Vec::with_capacity(policies.len());
        for policy in policies {
            let (severity, title, body) = match payload.status {
                GatewayCertificateRenewalStatus::Failed => match payload.failure_kind {
                    Some(GatewayCertificateRenewalFailureKind::Rejected) => (
                        NotificationSeverity::Warning,
                        "Gateway certificate renewal rejected".to_owned(),
                        format!(
                            "Certificate renewal for {} (Route {}) was rejected on Gateway node {}. The active certificate expires at {}.",
                            payload.hostname, payload.route_id, payload.node_id, expires_at
                        ),
                    ),
                    Some(GatewayCertificateRenewalFailureKind::Unavailable) => (
                        NotificationSeverity::Critical,
                        "Gateway certificate renewal unavailable".to_owned(),
                        format!(
                            "Certificate renewal for {} (Route {}) is unavailable on Gateway node {}. The active certificate expires at {}.",
                            payload.hostname, payload.route_id, payload.node_id, expires_at
                        ),
                    ),
                    None => {
                        return Err(RepositoryError::Storage(
                            "notification Gateway certificate renewal failure kind is missing"
                                .into(),
                        ));
                    }
                },
                GatewayCertificateRenewalStatus::Renewed => {
                    if !policy.definition.spec().notify_on_recovery {
                        continue;
                    }
                    let latest = self
                        .notifications
                        .latest_alert_source_projection(
                            policy.organization_id,
                            policy.recipient_principal_id,
                            source,
                            message.aggregate_id,
                            policy.created_at,
                            message.aggregate_version,
                        )
                        .await?;
                    if latest
                        .as_ref()
                        .map(|notification| notification.source_event_key.as_str())
                        != Some("edge.gateway-certificate.renewal-failed")
                    {
                        continue;
                    }
                    (
                        NotificationSeverity::Information,
                        "Gateway certificate renewal recovered".to_owned(),
                        format!(
                            "Certificate renewal for {} (Route {}) recovered on Gateway node {}. The active certificate now expires at {}.",
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

#[async_trait]
impl IIntegrationEventProjector for OutboxNotificationProjector {
    async fn project(&self, message: &OutboxMessage) -> Result<(), RepositoryError> {
        for notification in self.notifications_for(message).await? {
            self.notifications.project(notification).await?;
        }
        Ok(())
    }
}

fn decode_domain_claim(message: &OutboxMessage) -> Result<DomainClaimChanged, RepositoryError> {
    let payload: DomainClaimChanged =
        serde_json::from_value(message.payload.clone()).map_err(|error| {
            RepositoryError::Storage(format!(
                "notification source domain claim payload is invalid: {error}"
            ))
        })?;
    let expected_state = match message.event_key.as_str() {
        "edge.domain-claim.rejected" => DomainClaimState::Rejected,
        "edge.domain-claim.verified" => DomainClaimState::Verified,
        _ => {
            return Err(RepositoryError::Storage(
                "notification domain claim source key is unsupported".into(),
            ))
        }
    };
    let valid_failure = match expected_state {
        DomainClaimState::Rejected => payload.failure.as_deref().is_some_and(|failure| {
            !failure.is_empty()
                && failure.len() <= 4_096
                && failure.trim() == failure
                && !failure.contains(['\0', '\r', '\n'])
        }),
        DomainClaimState::Verified => payload.failure.is_none(),
        DomainClaimState::Pending | DomainClaimState::Revoked => false,
    };
    if payload.organization_id.as_uuid() != message.organization_id
        || payload.domain_claim_id.as_uuid() != message.aggregate_id
        || payload.project_id.as_uuid().is_nil()
        || payload.environment_id.as_uuid().is_nil()
        || payload.state != expected_state
        || message.aggregate_version < 2
        || DomainNamePattern::parse(payload.pattern.clone()).is_err()
        || !valid_failure
    {
        return Err(RepositoryError::Storage(
            "notification source domain claim payload identity is inconsistent".into(),
        ));
    }
    Ok(payload)
}

fn decode_gateway_certificate_renewal(
    message: &OutboxMessage,
) -> Result<GatewayCertificateRenewalChanged, RepositoryError> {
    let payload: GatewayCertificateRenewalChanged = serde_json::from_value(message.payload.clone())
        .map_err(|error| {
            RepositoryError::Storage(format!(
                "notification source Gateway certificate renewal payload is invalid: {error}"
            ))
        })?;
    let (expected_status, expected_failure_kind) = match message.event_key.as_str() {
        "edge.gateway-certificate.renewal-failed" => (
            GatewayCertificateRenewalStatus::Failed,
            Some(payload.failure_kind.ok_or_else(|| {
                RepositoryError::Storage(
                    "notification Gateway certificate renewal failure kind is missing".into(),
                )
            })?),
        ),
        "edge.gateway-certificate.renewed" => (GatewayCertificateRenewalStatus::Renewed, None),
        _ => {
            return Err(RepositoryError::Storage(
                "notification Gateway certificate renewal source key is unsupported".into(),
            ));
        }
    };
    let hostname = RouteHostname::parse(payload.hostname.clone()).map_err(|_| {
        RepositoryError::Storage(
            "notification Gateway certificate renewal hostname is invalid".into(),
        )
    })?;
    let path_prefix = RoutePath::parse(payload.path_prefix.clone()).map_err(|_| {
        RepositoryError::Storage("notification Gateway certificate renewal path is invalid".into())
    })?;
    let expected_active_certificate_id = match expected_status {
        GatewayCertificateRenewalStatus::Failed => payload.previous_certificate_id,
        GatewayCertificateRenewalStatus::Renewed => payload.replacement_certificate_id,
    };
    if payload.organization_id.as_uuid() != message.organization_id
        || payload.project_id.as_uuid().is_nil()
        || payload.environment_id.as_uuid().is_nil()
        || payload.route_id.as_uuid().is_nil()
        || payload.workload_id.as_uuid().is_nil()
        || payload.node_id.as_uuid().is_nil()
        || payload.previous_certificate_id.as_uuid().is_nil()
        || payload.replacement_certificate_id.as_uuid().is_nil()
        || payload.active_certificate_id.as_uuid().is_nil()
        || payload.previous_certificate_id == payload.replacement_certificate_id
        || payload.gateway_revision == 0
        || payload.gateway_revision != message.aggregate_version
        || renewal_subject_id(payload.route_id, payload.node_id) != message.aggregate_id
        || hostname.as_str() != payload.hostname
        || path_prefix.as_str() != payload.path_prefix
        || payload.active_certificate_expires_at
            != canonical_timestamp(payload.active_certificate_expires_at)
        || payload.status != expected_status
        || payload.failure_kind != expected_failure_kind
        || payload.active_certificate_id != expected_active_certificate_id
    {
        return Err(RepositoryError::Storage(
            "notification source Gateway certificate renewal payload identity is inconsistent"
                .into(),
        ));
    }
    Ok(payload)
}

fn decode_membership(message: &OutboxMessage) -> Result<MembershipChanged, RepositoryError> {
    let payload: MembershipChanged =
        serde_json::from_value(message.payload.clone()).map_err(|error| {
            RepositoryError::Storage(format!(
                "notification source membership payload is invalid: {error}"
            ))
        })?;
    validate_identity_payload(
        message,
        payload.membership_id,
        payload.principal_id,
        &payload.role,
        "membership",
    )?;
    Ok(payload)
}

fn validate_identity_payload(
    message: &OutboxMessage,
    aggregate_id: uuid::Uuid,
    principal_id: uuid::Uuid,
    role: &str,
    label: &str,
) -> Result<(), RepositoryError> {
    if aggregate_id.is_nil()
        || aggregate_id != message.aggregate_id
        || principal_id.is_nil()
        || MembershipRole::parse(role).is_err()
    {
        return Err(RepositoryError::Storage(format!(
            "notification source {label} payload identity is inconsistent"
        )));
    }
    Ok(())
}

#[cfg(test)]
#[path = "outbox_projector_tests.rs"]
mod tests;
