use super::{NotificationAlertPolicy, NotificationAlertPolicyCursor, NotificationAlertSource};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, IdempotencyRequest, IdempotentWrite, NotificationAlertPolicyId, OrganizationId,
    PrincipalId, ProjectId, RepositoryError,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CreateNotificationAlertPolicyWrite {
    pub policy: NotificationAlertPolicy,
    pub event: DomainEventEnvelope,
    pub actor_principal_id: PrincipalId,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

impl CreateNotificationAlertPolicyWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.policy.validate()?;
        self.idempotency.validate()?;
        validate_policy_event(
            &self.policy,
            &self.event,
            self.actor_principal_id,
            self.request_id,
            "notification.alert-policy.created",
        )?;
        if self.policy.aggregate_version != 1 || !self.policy.is_active() {
            return Err("notification alert policy create write is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct RevokeNotificationAlertPolicyWrite {
    pub policy: NotificationAlertPolicy,
    pub expected_version: u64,
    pub event: DomainEventEnvelope,
    pub actor_principal_id: PrincipalId,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

impl RevokeNotificationAlertPolicyWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.policy.validate()?;
        self.idempotency.validate()?;
        validate_policy_event(
            &self.policy,
            &self.event,
            self.actor_principal_id,
            self.request_id,
            "notification.alert-policy.revoked",
        )?;
        if self.expected_version != 1
            || self.policy.aggregate_version != self.expected_version + 1
            || self.policy.is_active()
        {
            return Err("notification alert policy revoke write is invalid".into());
        }
        Ok(())
    }

    pub fn validate_against(&self, existing: &NotificationAlertPolicy) -> Result<(), String> {
        self.validate()?;
        let expected = existing.revoke(
            self.expected_version,
            self.actor_principal_id,
            self.policy
                .revoked_at
                .expect("validated policy revoke time"),
        )?;
        if expected != self.policy {
            return Err("notification alert policy revoke transition changed".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NotificationAlertPolicyEvent {
    pub policy_id: NotificationAlertPolicyId,
    pub recipient_principal_id: PrincipalId,
    pub definition_digest: String,
    pub source: String,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub notify_on_recovery: bool,
    pub state: String,
}

impl NotificationAlertPolicyEvent {
    pub fn envelope(
        event_key: &str,
        policy: &NotificationAlertPolicy,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, String> {
        policy.validate()?;
        let spec = policy.definition.spec();
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: event_key.into(),
            schema_version: 1,
            organization_id: policy.organization_id.as_uuid(),
            aggregate_id: policy.id.as_uuid(),
            aggregate_version: policy.aggregate_version,
            occurred_at: policy.revoked_at.unwrap_or(policy.created_at),
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(Self {
                policy_id: policy.id,
                recipient_principal_id: policy.recipient_principal_id,
                definition_digest: policy.definition.digest().to_string(),
                source: spec.source.as_str().into(),
                project_id: spec.project_id,
                environment_id: spec.environment_id,
                notify_on_recovery: spec.notify_on_recovery,
                state: if policy.is_active() {
                    "active".into()
                } else {
                    "revoked".into()
                },
            })
            .map_err(|error| error.to_string())?,
        })
    }
}

#[async_trait]
pub trait INotificationAlertPolicyRepository: Send + Sync {
    async fn replay_alert_policy_write(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<IdempotentWrite<NotificationAlertPolicy>>, RepositoryError>;

    async fn create_alert_policy(
        &self,
        write: CreateNotificationAlertPolicyWrite,
    ) -> Result<IdempotentWrite<NotificationAlertPolicy>, RepositoryError>;

    async fn revoke_alert_policy(
        &self,
        write: RevokeNotificationAlertPolicyWrite,
    ) -> Result<IdempotentWrite<NotificationAlertPolicy>, RepositoryError>;

    async fn find_alert_policy(
        &self,
        organization_id: OrganizationId,
        recipient_principal_id: PrincipalId,
        policy_id: NotificationAlertPolicyId,
    ) -> Result<Option<NotificationAlertPolicy>, RepositoryError>;

    async fn list_alert_policy_page(
        &self,
        organization_id: OrganizationId,
        recipient_principal_id: PrincipalId,
        after: Option<NotificationAlertPolicyCursor>,
        limit: usize,
    ) -> Result<Vec<NotificationAlertPolicy>, RepositoryError>;

    async fn list_active_alert_policies_for_source(
        &self,
        organization_id: OrganizationId,
        source: NotificationAlertSource,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        occurred_at: DateTime<Utc>,
    ) -> Result<Vec<NotificationAlertPolicy>, RepositoryError>;
}

fn validate_policy_event(
    policy: &NotificationAlertPolicy,
    event: &DomainEventEnvelope,
    actor_principal_id: PrincipalId,
    request_id: Uuid,
    event_key: &str,
) -> Result<(), String> {
    if actor_principal_id != policy.recipient_principal_id
        || actor_principal_id.as_uuid().is_nil()
        || request_id.is_nil()
        || event.event_key != event_key
        || event.schema_version != 1
        || event.organization_id != policy.organization_id.as_uuid()
        || event.aggregate_id != policy.id.as_uuid()
        || event.aggregate_version != policy.aggregate_version
        || event.occurred_at != policy.revoked_at.unwrap_or(policy.created_at)
        || event.correlation_id != request_id
        || event.causation_id.is_some()
    {
        return Err("notification alert policy event identity is inconsistent".into());
    }
    let payload: NotificationAlertPolicyEvent = serde_json::from_value(event.payload.clone())
        .map_err(|error| format!("notification alert policy event is invalid: {error}"))?;
    let spec = policy.definition.spec();
    if payload.policy_id != policy.id
        || payload.recipient_principal_id != policy.recipient_principal_id
        || payload.definition_digest != policy.definition.digest().as_str()
        || payload.source != spec.source.as_str()
        || payload.project_id != spec.project_id
        || payload.environment_id != spec.environment_id
        || payload.notify_on_recovery != spec.notify_on_recovery
        || payload.state
            != if policy.is_active() {
                "active"
            } else {
                "revoked"
            }
    {
        return Err("notification alert policy event payload is inconsistent".into());
    }
    Ok(())
}
