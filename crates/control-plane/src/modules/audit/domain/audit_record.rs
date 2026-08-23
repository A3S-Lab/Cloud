use crate::modules::shared_kernel::domain::{
    validate_audit_action, EnvironmentId, OrganizationId, PrincipalId, ProjectAttributionProfileId,
    ProjectId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const CURSOR_VERSION: &str = "v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditRecord {
    pub id: Uuid,
    pub organization_id: OrganizationId,
    pub actor_principal_id: Option<PrincipalId>,
    pub action: String,
    pub aggregate_id: Uuid,
    pub occurred_at: DateTime<Utc>,
    pub request_id: Uuid,
    pub project_id: Option<ProjectId>,
    pub environment_id: Option<EnvironmentId>,
    pub attribution_profile_id: Option<ProjectAttributionProfileId>,
    pub attribution_status: AuditAttributionStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditAttributionStatus {
    LegacyUnknown,
    NotApplicable,
    ProfileMissing,
    ProfileBound,
}

impl AuditAttributionStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LegacyUnknown => "legacy_unknown",
            Self::NotApplicable => "not_applicable",
            Self::ProfileMissing => "profile_missing",
            Self::ProfileBound => "profile_bound",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "legacy_unknown" => Ok(Self::LegacyUnknown),
            "not_applicable" => Ok(Self::NotApplicable),
            "profile_missing" => Ok(Self::ProfileMissing),
            "profile_bound" => Ok(Self::ProfileBound),
            _ => Err("audit attribution status is invalid".into()),
        }
    }
}

impl AuditRecord {
    pub fn validate(&self) -> Result<(), String> {
        if self.id.is_nil()
            || self.organization_id.as_uuid().is_nil()
            || self
                .actor_principal_id
                .is_some_and(|actor| actor.as_uuid().is_nil())
            || self.aggregate_id.is_nil()
            || self.request_id.is_nil()
            || self
                .project_id
                .is_some_and(|value| value.as_uuid().is_nil())
            || self
                .environment_id
                .is_some_and(|value| value.as_uuid().is_nil())
            || self
                .attribution_profile_id
                .is_some_and(|value| value.as_uuid().is_nil())
        {
            return Err("audit record identifiers must not be nil".into());
        }
        validate_audit_action(&self.action)?;
        let attribution_shape_is_valid = match self.attribution_status {
            AuditAttributionStatus::LegacyUnknown | AuditAttributionStatus::NotApplicable => {
                self.project_id.is_none()
                    && self.environment_id.is_none()
                    && self.attribution_profile_id.is_none()
            }
            AuditAttributionStatus::ProfileMissing => {
                self.project_id.is_some() && self.attribution_profile_id.is_none()
            }
            AuditAttributionStatus::ProfileBound => {
                self.project_id.is_some() && self.attribution_profile_id.is_some()
            }
        };
        if !attribution_shape_is_valid {
            return Err("audit attribution shape is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuditRecordCursor {
    pub occurred_at: DateTime<Utc>,
    pub audit_id: Uuid,
}

impl AuditRecordCursor {
    pub fn after(record: &AuditRecord) -> Self {
        Self {
            occurred_at: record.occurred_at,
            audit_id: record.id,
        }
    }

    pub fn encode(self) -> String {
        format!(
            "{CURSOR_VERSION}:{}:{}",
            self.occurred_at.timestamp_micros(),
            self.audit_id
        )
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        if value.is_empty() || value.len() > 128 || value.contains(['\0', '\r', '\n']) {
            return Err("audit record cursor is invalid".into());
        }
        let mut parts = value.split(':');
        let version = parts.next();
        let timestamp = parts.next();
        let audit_id = parts.next();
        if version != Some(CURSOR_VERSION) || parts.next().is_some() {
            return Err("audit record cursor is invalid".into());
        }
        let occurred_at = timestamp
            .and_then(|value| value.parse::<i64>().ok())
            .and_then(DateTime::<Utc>::from_timestamp_micros)
            .ok_or_else(|| "audit record cursor is invalid".to_owned())?;
        let audit_id = audit_id
            .and_then(|value| Uuid::parse_str(value).ok())
            .filter(|value| !value.is_nil())
            .ok_or_else(|| "audit record cursor is invalid".to_owned())?;
        Ok(Self {
            occurred_at,
            audit_id,
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuditRecordFilter {
    pub actor_principal_id: Option<PrincipalId>,
    pub action: Option<String>,
    pub aggregate_id: Option<Uuid>,
    pub request_id: Option<Uuid>,
    pub project_id: Option<ProjectId>,
    pub environment_id: Option<EnvironmentId>,
    pub attribution_profile_id: Option<ProjectAttributionProfileId>,
    pub attribution_status: Option<AuditAttributionStatus>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
}

impl AuditRecordFilter {
    pub fn validate(&self) -> Result<(), String> {
        if self
            .actor_principal_id
            .is_some_and(|value| value.as_uuid().is_nil())
            || self.aggregate_id.is_some_and(|value| value.is_nil())
            || self.request_id.is_some_and(|value| value.is_nil())
            || self
                .project_id
                .is_some_and(|value| value.as_uuid().is_nil())
            || self
                .environment_id
                .is_some_and(|value| value.as_uuid().is_nil())
            || self
                .attribution_profile_id
                .is_some_and(|value| value.as_uuid().is_nil())
        {
            return Err("audit record filter identifiers must not be nil".into());
        }
        if let Some(action) = &self.action {
            validate_audit_action(action)?;
        }
        if self.from.zip(self.to).is_some_and(|(from, to)| from > to) {
            return Err("audit record from timestamp must not exceed to timestamp".into());
        }
        Ok(())
    }

    pub fn matches(&self, record: &AuditRecord) -> bool {
        self.actor_principal_id
            .is_none_or(|value| record.actor_principal_id == Some(value))
            && self
                .action
                .as_ref()
                .is_none_or(|value| &record.action == value)
            && self
                .aggregate_id
                .is_none_or(|value| record.aggregate_id == value)
            && self
                .request_id
                .is_none_or(|value| record.request_id == value)
            && self
                .project_id
                .is_none_or(|value| record.project_id == Some(value))
            && self
                .environment_id
                .is_none_or(|value| record.environment_id == Some(value))
            && self
                .attribution_profile_id
                .is_none_or(|value| record.attribution_profile_id == Some(value))
            && self
                .attribution_status
                .is_none_or(|value| record.attribution_status == value)
            && self.from.is_none_or(|value| record.occurred_at >= value)
            && self.to.is_none_or(|value| record.occurred_at <= value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditRecordPage {
    pub records: Vec<AuditRecord>,
    pub next_cursor: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn cursor_round_trip_preserves_postgres_precision_and_rejects_untrusted_values() {
        let cursor = AuditRecordCursor {
            occurred_at: Utc
                .timestamp_opt(1_800_000_000, 123_456_000)
                .single()
                .expect("timestamp"),
            audit_id: Uuid::now_v7(),
        };
        assert_eq!(AuditRecordCursor::parse(&cursor.encode()), Ok(cursor));
        for invalid in [
            "",
            "v2:1:00000000-0000-0000-0000-000000000001",
            "v1:not-a-time:00000000-0000-0000-0000-000000000001",
            "v1:1:00000000-0000-0000-0000-000000000000",
            "v1:1:not-a-uuid",
            "v1:1:00000000-0000-0000-0000-000000000001:extra",
        ] {
            assert!(AuditRecordCursor::parse(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn filter_validates_action_identifiers_and_time_range() {
        let mut filter = AuditRecordFilter {
            action: Some("identity.membership.created".into()),
            ..AuditRecordFilter::default()
        };
        assert_eq!(filter.validate(), Ok(()));
        filter.action = Some("Identity created".into());
        assert!(filter.validate().is_err());
        filter.action = None;
        filter.aggregate_id = Some(Uuid::nil());
        assert!(filter.validate().is_err());
        filter.aggregate_id = None;
        filter.from = Some(Utc::now());
        filter.to = filter
            .from
            .map(|value| value - chrono::Duration::seconds(1));
        assert!(filter.validate().is_err());
    }

    #[test]
    fn attribution_status_and_reference_shapes_are_closed() {
        for status in [
            AuditAttributionStatus::LegacyUnknown,
            AuditAttributionStatus::NotApplicable,
            AuditAttributionStatus::ProfileMissing,
            AuditAttributionStatus::ProfileBound,
        ] {
            assert_eq!(AuditAttributionStatus::parse(status.as_str()), Ok(status));
        }
        assert!(AuditAttributionStatus::parse("current_profile").is_err());

        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let profile_id = ProjectAttributionProfileId::new();
        for (status, project_id, environment_id, profile_id) in [
            (AuditAttributionStatus::LegacyUnknown, None, None, None),
            (AuditAttributionStatus::NotApplicable, None, None, None),
            (
                AuditAttributionStatus::ProfileMissing,
                Some(project_id),
                Some(environment_id),
                None,
            ),
            (
                AuditAttributionStatus::ProfileBound,
                Some(project_id),
                Some(environment_id),
                Some(profile_id),
            ),
        ] {
            assert_eq!(
                audit_record(status, project_id, environment_id, profile_id).validate(),
                Ok(())
            );
        }
        assert!(audit_record(
            AuditAttributionStatus::NotApplicable,
            Some(project_id),
            None,
            None,
        )
        .validate()
        .is_err());
        assert!(audit_record(
            AuditAttributionStatus::ProfileMissing,
            None,
            Some(environment_id),
            None,
        )
        .validate()
        .is_err());
        assert!(audit_record(
            AuditAttributionStatus::ProfileBound,
            Some(project_id),
            None,
            None,
        )
        .validate()
        .is_err());
    }

    fn audit_record(
        attribution_status: AuditAttributionStatus,
        project_id: Option<ProjectId>,
        environment_id: Option<EnvironmentId>,
        attribution_profile_id: Option<ProjectAttributionProfileId>,
    ) -> AuditRecord {
        AuditRecord {
            id: Uuid::now_v7(),
            organization_id: OrganizationId::new(),
            actor_principal_id: None,
            action: "identity.membership.created".into(),
            aggregate_id: Uuid::now_v7(),
            occurred_at: Utc::now(),
            request_id: Uuid::now_v7(),
            project_id,
            environment_id,
            attribution_profile_id,
            attribution_status,
        }
    }
}
