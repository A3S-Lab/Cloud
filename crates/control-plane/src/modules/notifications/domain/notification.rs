use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::identity::domain::value_objects::ResourceGrantScope;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, NodeId, NotificationId, OrganizationId, PrincipalId,
    ProjectId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const CURSOR_VERSION: &str = "v1";
const MAXIMUM_TITLE_CHARACTERS: usize = 160;
const MAXIMUM_BODY_CHARACTERS: usize = 2_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationSeverity {
    Information,
    Warning,
    Critical,
}

impl NotificationSeverity {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Information => "information",
            Self::Warning => "warning",
            Self::Critical => "critical",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "information" => Ok(Self::Information),
            "warning" => Ok(Self::Warning),
            "critical" => Ok(Self::Critical),
            _ => Err("notification severity is invalid".into()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NotificationScope {
    Organization,
    Project {
        project_id: ProjectId,
    },
    Environment {
        project_id: ProjectId,
        environment_id: EnvironmentId,
    },
    Node {
        node_id: NodeId,
    },
}

impl NotificationScope {
    pub const fn kind(self) -> &'static str {
        match self {
            Self::Organization => "organization",
            Self::Project { .. } => "project",
            Self::Environment { .. } => "environment",
            Self::Node { .. } => "node",
        }
    }

    pub const fn project_id(self) -> Option<ProjectId> {
        match self {
            Self::Project { project_id } | Self::Environment { project_id, .. } => Some(project_id),
            Self::Organization | Self::Node { .. } => None,
        }
    }

    pub const fn environment_id(self) -> Option<EnvironmentId> {
        match self {
            Self::Environment { environment_id, .. } => Some(environment_id),
            Self::Organization | Self::Project { .. } | Self::Node { .. } => None,
        }
    }

    pub const fn node_id(self) -> Option<NodeId> {
        match self {
            Self::Node { node_id } => Some(node_id),
            Self::Organization | Self::Project { .. } | Self::Environment { .. } => None,
        }
    }

    pub fn is_visible_to(self, evaluator: &ResourceAccessEvaluator) -> bool {
        match self {
            Self::Organization => true,
            Self::Project { project_id } => {
                evaluator.allows(ResourceGrantScope::Project { project_id })
            }
            Self::Environment {
                project_id,
                environment_id,
            } => evaluator.allows(ResourceGrantScope::Environment {
                project_id,
                environment_id,
            }),
            Self::Node { node_id } => evaluator.allows(ResourceGrantScope::Node { node_id }),
        }
    }

    fn validate(self) -> Result<(), String> {
        if self.project_id().is_some_and(|id| id.as_uuid().is_nil())
            || self
                .environment_id()
                .is_some_and(|id| id.as_uuid().is_nil())
            || self.node_id().is_some_and(|id| id.as_uuid().is_nil())
        {
            return Err("notification resource scope identifiers must not be nil".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notification {
    pub organization_id: OrganizationId,
    pub id: NotificationId,
    pub recipient_principal_id: PrincipalId,
    pub source_event_id: Uuid,
    pub source_event_key: String,
    pub source_schema_version: u32,
    pub source_aggregate_id: Uuid,
    pub source_aggregate_version: u64,
    pub correlation_id: Uuid,
    pub severity: NotificationSeverity,
    pub title: String,
    pub body: String,
    pub scope: NotificationScope,
    pub occurred_at: DateTime<Utc>,
    pub delivered_at: DateTime<Utc>,
    pub aggregate_version: u64,
    pub read_at: Option<DateTime<Utc>>,
}

impl Notification {
    #[allow(clippy::too_many_arguments)]
    pub fn project(
        organization_id: OrganizationId,
        recipient_principal_id: PrincipalId,
        source_event_id: Uuid,
        source_event_key: String,
        source_schema_version: u32,
        source_aggregate_id: Uuid,
        source_aggregate_version: u64,
        correlation_id: Uuid,
        severity: NotificationSeverity,
        title: String,
        body: String,
        scope: NotificationScope,
        occurred_at: DateTime<Utc>,
        delivered_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let id = NotificationId::from_uuid(Uuid::new_v5(
            &source_event_id,
            recipient_principal_id.as_uuid().as_bytes(),
        ));
        let notification = Self {
            organization_id,
            id,
            recipient_principal_id,
            source_event_id,
            source_event_key,
            source_schema_version,
            source_aggregate_id,
            source_aggregate_version,
            correlation_id,
            severity,
            title,
            body,
            scope,
            occurred_at: canonical_timestamp(occurred_at),
            delivered_at: canonical_timestamp(delivered_at),
            aggregate_version: 1,
            read_at: None,
        };
        notification.validate()?;
        Ok(notification)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.id.as_uuid().is_nil()
            || self.recipient_principal_id.as_uuid().is_nil()
            || self.source_event_id.is_nil()
            || self.source_aggregate_id.is_nil()
            || self.correlation_id.is_nil()
            || self.source_schema_version == 0
            || self.source_aggregate_version == 0
            || self.occurred_at != canonical_timestamp(self.occurred_at)
            || self.delivered_at != canonical_timestamp(self.delivered_at)
            || self.delivered_at < self.occurred_at
            || self.read_at != self.read_at.map(canonical_timestamp)
        {
            return Err("notification identifiers, versions, or timestamps are invalid".into());
        }
        let expected_id = Uuid::new_v5(
            &self.source_event_id,
            self.recipient_principal_id.as_uuid().as_bytes(),
        );
        if self.id.as_uuid() != expected_id {
            return Err(
                "notification identity must derive from its source event and recipient".into(),
            );
        }
        validate_event_key(&self.source_event_key)?;
        validate_text("notification title", &self.title, MAXIMUM_TITLE_CHARACTERS)?;
        validate_text("notification body", &self.body, MAXIMUM_BODY_CHARACTERS)?;
        self.scope.validate()?;
        match (self.aggregate_version, self.read_at) {
            (1, None) => Ok(()),
            (2, Some(read_at)) if read_at >= self.delivered_at => Ok(()),
            _ => Err("notification read state is invalid".into()),
        }
    }

    pub fn mark_read(&self, expected_version: u64, read_at: DateTime<Utc>) -> Result<Self, String> {
        self.validate()?;
        if self.aggregate_version != expected_version {
            return Err("notification version changed".into());
        }
        if self.read_at.is_some() {
            return Err("notification is already read".into());
        }
        let mut next = self.clone();
        next.aggregate_version = self.aggregate_version + 1;
        next.read_at = Some(canonical_timestamp(read_at).max(self.delivered_at));
        next.validate()?;
        Ok(next)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotificationCursor {
    pub occurred_at: DateTime<Utc>,
    pub notification_id: NotificationId,
}

impl NotificationCursor {
    pub fn after(notification: &Notification) -> Self {
        Self {
            occurred_at: notification.occurred_at,
            notification_id: notification.id,
        }
    }

    pub fn encode(self) -> String {
        encode_descending_cursor(self.occurred_at, self.notification_id.as_uuid())
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        let (occurred_at, notification_id) = parse_descending_cursor(value, "notification")?;
        Ok(Self {
            occurred_at,
            notification_id: NotificationId::from_uuid(notification_id),
        })
    }
}

pub(super) fn encode_descending_cursor(occurred_at: DateTime<Utc>, id: Uuid) -> String {
    format!("{CURSOR_VERSION}:{}:{id}", occurred_at.timestamp_micros())
}

pub(super) fn parse_descending_cursor(
    value: &str,
    label: &str,
) -> Result<(DateTime<Utc>, Uuid), String> {
    let invalid = || format!("{label} cursor is invalid");
    if value.is_empty() || value.len() > 128 || value.contains(['\0', '\r', '\n']) {
        return Err(invalid());
    }
    let mut parts = value.split(':');
    let version = parts.next();
    let timestamp = parts.next();
    let id = parts.next();
    if version != Some(CURSOR_VERSION) || parts.next().is_some() {
        return Err(invalid());
    }
    let occurred_at = timestamp
        .and_then(|value| value.parse::<i64>().ok())
        .and_then(DateTime::<Utc>::from_timestamp_micros)
        .ok_or_else(invalid)?;
    let id = id
        .and_then(|value| Uuid::parse_str(value).ok())
        .filter(|value| !value.is_nil())
        .ok_or_else(invalid)?;
    Ok((occurred_at, id))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationPage {
    pub notifications: Vec<Notification>,
    pub next_cursor: Option<String>,
}

fn validate_event_key(value: &str) -> Result<(), String> {
    let valid = !value.is_empty()
        && value.len() <= 255
        && value.split('.').count() >= 3
        && value.split('.').all(|segment| {
            segment
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_lowercase)
                && segment
                    .as_bytes()
                    .last()
                    .is_some_and(u8::is_ascii_lowercase)
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte == b'-')
        });
    if valid {
        Ok(())
    } else {
        Err("notification source event key is invalid".into())
    }
}

fn validate_text(label: &str, value: &str, maximum_characters: usize) -> Result<(), String> {
    if value.trim() != value
        || value.is_empty()
        || value.chars().count() > maximum_characters
        || value.chars().any(char::is_control)
    {
        return Err(format!(
            "{label} must be trimmed, visible, and at most {maximum_characters} characters"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notification(scope: NotificationScope) -> Notification {
        let now = Utc::now();
        Notification::project(
            OrganizationId::new(),
            PrincipalId::new(),
            Uuid::now_v7(),
            "identity.membership.role-changed".into(),
            1,
            Uuid::now_v7(),
            2,
            Uuid::now_v7(),
            NotificationSeverity::Information,
            "Organization role changed".into(),
            "Your organization role is now member.".into(),
            scope,
            now,
            now,
        )
        .expect("notification")
    }

    #[test]
    fn identity_is_deterministic_and_read_is_one_way() {
        let original = notification(NotificationScope::Organization);
        let replay = Notification::project(
            original.organization_id,
            original.recipient_principal_id,
            original.source_event_id,
            original.source_event_key.clone(),
            original.source_schema_version,
            original.source_aggregate_id,
            original.source_aggregate_version,
            original.correlation_id,
            original.severity,
            original.title.clone(),
            original.body.clone(),
            original.scope,
            original.occurred_at,
            original.delivered_at,
        )
        .expect("replay");
        assert_eq!(replay.id, original.id);
        let read = original
            .mark_read(1, original.delivered_at)
            .expect("mark read");
        assert_eq!(read.aggregate_version, 2);
        assert!(read.read_at.is_some());
        assert!(read.mark_read(2, Utc::now()).is_err());
    }

    #[test]
    fn scopes_reuse_the_shared_grant_evaluator() {
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let evaluator = ResourceAccessEvaluator::restricted([ResourceGrantScope::Environment {
            project_id,
            environment_id,
        }]);
        assert!(NotificationScope::Organization.is_visible_to(&evaluator));
        assert!(NotificationScope::Environment {
            project_id,
            environment_id
        }
        .is_visible_to(&evaluator));
        assert!(!NotificationScope::Project { project_id }.is_visible_to(&evaluator));
        assert!(!NotificationScope::Node {
            node_id: NodeId::new()
        }
        .is_visible_to(&evaluator));
    }

    #[test]
    fn cursors_and_visible_text_are_bounded() {
        let notification = notification(NotificationScope::Organization);
        let cursor = NotificationCursor::after(&notification);
        assert_eq!(NotificationCursor::parse(&cursor.encode()), Ok(cursor));
        assert!(NotificationCursor::parse("invalid").is_err());
        let mut invalid = notification;
        invalid.body = " secret\nvalue ".into();
        assert!(invalid.validate().is_err());
        assert!(validate_event_key("identity.-membership.changed").is_err());
        assert!(validate_event_key("identity.membership-.changed").is_err());
    }
}
