use crate::modules::notifications::domain::{
    Notification, NotificationPage, NotificationScope, NotificationSeverity,
};
use crate::modules::notifications::MarkNotificationReadResult;
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
