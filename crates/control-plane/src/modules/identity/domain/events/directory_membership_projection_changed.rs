use crate::modules::identity::domain::value_objects::DirectoryGrantSubjectRef;
use crate::modules::shared_kernel::domain::{OrganizationId, PrincipalId};
use a3s_cloud_contracts::DomainEventEnvelope;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryMembershipProjectionChanged {
    pub subject_kind: String,
    pub issuer: String,
    pub subject_id: Uuid,
    pub principal_ids: Vec<Uuid>,
}

impl DirectoryMembershipProjectionChanged {
    pub fn replaced(
        organization_id: OrganizationId,
        subject: &DirectoryGrantSubjectRef,
        principal_ids: &[PrincipalId],
        occurred_at: DateTime<Utc>,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Self::envelope(
            "identity.directory-membership-projection.replaced",
            organization_id,
            subject,
            principal_ids,
            occurred_at,
            correlation_id,
        )
    }

    pub fn cleared(
        organization_id: OrganizationId,
        subject: &DirectoryGrantSubjectRef,
        occurred_at: DateTime<Utc>,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Self::envelope(
            "identity.directory-membership-projection.cleared",
            organization_id,
            subject,
            &[],
            occurred_at,
            correlation_id,
        )
    }

    fn envelope(
        event_key: &str,
        organization_id: OrganizationId,
        subject: &DirectoryGrantSubjectRef,
        principal_ids: &[PrincipalId],
        occurred_at: DateTime<Utc>,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        let mut principal_ids = principal_ids
            .iter()
            .map(|id| id.as_uuid())
            .collect::<Vec<_>>();
        principal_ids.sort_unstable();
        principal_ids.dedup();
        let payload = Self {
            subject_kind: subject.kind().as_str().to_owned(),
            issuer: subject.issuer().as_str().to_owned(),
            subject_id: subject.subject_id(),
            principal_ids: principal_ids.clone(),
        };
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: event_key.into(),
            schema_version: 1,
            scope: a3s_cloud_contracts::CloudScopeRef::Organization {
                organization_id: organization_id.as_uuid(),
            },
            aggregate_id: subject.subject_id(),
            aggregate_version: 1,
            occurred_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(payload)?,
        })
    }
}
