use crate::modules::notifications::domain::{
    Notification, NotificationPage, NotificationScope, NotificationSeverity,
    OutboundNotificationChannel, OutboundNotificationSubscription,
    OutboundNotificationSubscriptionPage,
};
use crate::modules::notifications::{
    MarkNotificationReadResult, OutboundNotificationSubscriptionMutationResult,
    OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationResponse {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub source_event_id: Uuid,
    pub source_event_key: String,
    pub source_aggregate_id: Uuid,
    pub severity: NotificationSeverity,
    pub title: String,
    pub body: String,
    pub scope: NotificationScopeResponse,
    pub occurred_at: DateTime<Utc>,
    pub delivered_at: DateTime<Utc>,
    pub aggregate_version: u64,
    pub read_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum NotificationScopeResponse {
    Organization,
    Project {
        project_id: Uuid,
    },
    Environment {
        project_id: Uuid,
        environment_id: Uuid,
    },
    Node {
        node_id: Uuid,
    },
}

impl From<NotificationScope> for NotificationScopeResponse {
    fn from(scope: NotificationScope) -> Self {
        match scope {
            NotificationScope::Organization => Self::Organization,
            NotificationScope::Project { project_id } => Self::Project {
                project_id: project_id.as_uuid(),
            },
            NotificationScope::Environment {
                project_id,
                environment_id,
            } => Self::Environment {
                project_id: project_id.as_uuid(),
                environment_id: environment_id.as_uuid(),
            },
            NotificationScope::Node { node_id } => Self::Node {
                node_id: node_id.as_uuid(),
            },
        }
    }
}

impl From<Notification> for NotificationResponse {
    fn from(notification: Notification) -> Self {
        Self {
            id: notification.id.as_uuid(),
            organization_id: notification.organization_id.as_uuid(),
            source_event_id: notification.source_event_id,
            source_event_key: notification.source_event_key,
            source_aggregate_id: notification.source_aggregate_id,
            severity: notification.severity,
            title: notification.title,
            body: notification.body,
            scope: notification.scope.into(),
            occurred_at: notification.occurred_at,
            delivered_at: notification.delivered_at,
            aggregate_version: notification.aggregate_version,
            read_at: notification.read_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationPageResponse {
    pub notifications: Vec<NotificationResponse>,
    pub next_cursor: Option<String>,
}

impl From<NotificationPage> for NotificationPageResponse {
    fn from(page: NotificationPage) -> Self {
        Self {
            notifications: page
                .notifications
                .into_iter()
                .map(NotificationResponse::from)
                .collect(),
            next_cursor: page.next_cursor,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationMutationResponse {
    pub notification: NotificationResponse,
    pub replayed: bool,
}

impl From<MarkNotificationReadResult> for NotificationMutationResponse {
    fn from(result: MarkNotificationReadResult) -> Self {
        Self {
            notification: result.notification.into(),
            replayed: result.replayed,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MarkNotificationReadRequest {
    pub expected_version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutboundNotificationSubscriptionResponse {
    pub organization_id: Uuid,
    pub subscription_id: Uuid,
    pub channel: OutboundNotificationChannel,
    pub minimum_severity: NotificationSeverity,
    pub connector_project_id: Uuid,
    pub connector_environment_id: Uuid,
    pub connector_profile_id: Uuid,
    pub connector_revision_id: Uuid,
    pub definition_schema: String,
    pub definition_acl: String,
    pub definition_digest: String,
    pub state: String,
    pub aggregate_version: u64,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

impl From<OutboundNotificationSubscription> for OutboundNotificationSubscriptionResponse {
    fn from(subscription: OutboundNotificationSubscription) -> Self {
        let spec = subscription.definition.spec();
        Self {
            organization_id: subscription.organization_id.as_uuid(),
            subscription_id: subscription.id.as_uuid(),
            channel: spec.channel,
            minimum_severity: spec.minimum_severity,
            connector_project_id: spec.target.project_id.as_uuid(),
            connector_environment_id: spec.target.environment_id.as_uuid(),
            connector_profile_id: spec.target.profile_id.as_uuid(),
            connector_revision_id: spec.target.revision_id.as_uuid(),
            definition_schema: OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA.into(),
            definition_acl: subscription.definition.canonical_acl().to_owned(),
            definition_digest: subscription.definition.digest().as_str().to_owned(),
            state: if subscription.is_active() {
                "active".into()
            } else {
                "revoked".into()
            },
            aggregate_version: subscription.aggregate_version,
            created_by: subscription.created_by.as_uuid(),
            created_at: subscription.created_at,
            revoked_at: subscription.revoked_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutboundNotificationSubscriptionPageResponse {
    pub subscriptions: Vec<OutboundNotificationSubscriptionResponse>,
    pub next_cursor: Option<String>,
}

impl From<OutboundNotificationSubscriptionPage> for OutboundNotificationSubscriptionPageResponse {
    fn from(page: OutboundNotificationSubscriptionPage) -> Self {
        Self {
            subscriptions: page
                .subscriptions
                .into_iter()
                .map(OutboundNotificationSubscriptionResponse::from)
                .collect(),
            next_cursor: page.next_cursor,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutboundNotificationSubscriptionMutationResponse {
    pub subscription: OutboundNotificationSubscriptionResponse,
    pub replayed: bool,
}

impl From<OutboundNotificationSubscriptionMutationResult>
    for OutboundNotificationSubscriptionMutationResponse
{
    fn from(result: OutboundNotificationSubscriptionMutationResult) -> Self {
        Self {
            subscription: result.subscription.into(),
            replayed: result.replayed,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RevokeOutboundNotificationSubscriptionRequest {
    pub expected_version: u64,
}
