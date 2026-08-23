use crate::modules::audit::domain::{
    AuditExport, AuditExportDsseEnvelope, AuditExportSigningKey, AuditRecord, AuditRecordPage,
    AuditRetentionStatus,
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditRecordResponse {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub actor_principal_id: Option<Uuid>,
    pub action: String,
    pub aggregate_id: Uuid,
    pub occurred_at: DateTime<Utc>,
    pub request_id: Uuid,
    pub project_id: Option<Uuid>,
    pub environment_id: Option<Uuid>,
    pub attribution_profile_id: Option<Uuid>,
    pub attribution_status: String,
}

impl From<AuditRecord> for AuditRecordResponse {
    fn from(record: AuditRecord) -> Self {
        Self {
            id: record.id,
            organization_id: record.organization_id.as_uuid(),
            actor_principal_id: record.actor_principal_id.map(|value| value.as_uuid()),
            action: record.action,
            aggregate_id: record.aggregate_id,
            occurred_at: record.occurred_at,
            request_id: record.request_id,
            project_id: record.project_id.map(|value| value.as_uuid()),
            environment_id: record.environment_id.map(|value| value.as_uuid()),
            attribution_profile_id: record.attribution_profile_id.map(|value| value.as_uuid()),
            attribution_status: record.attribution_status.as_str().into(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditRecordPageResponse {
    pub records: Vec<AuditRecordResponse>,
    pub next_cursor: Option<String>,
}

impl From<AuditRecordPage> for AuditRecordPageResponse {
    fn from(page: AuditRecordPage) -> Self {
        Self {
            records: page
                .records
                .into_iter()
                .map(AuditRecordResponse::from)
                .collect(),
            next_cursor: page.next_cursor,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditExportResponse {
    pub envelope: AuditExportDsseEnvelope,
    pub signing_key: AuditExportSigningKey,
}

impl From<AuditExport> for AuditExportResponse {
    fn from(export: AuditExport) -> Self {
        Self {
            envelope: export.envelope,
            signing_key: export.signing_key,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditRetentionStatusResponse {
    pub organization_id: Uuid,
    pub retention_ms: u64,
    pub policy_digest: String,
    pub applied_policy_digest: Option<String>,
    pub current_policy_applied: bool,
    pub records_available_from: Option<DateTime<Utc>>,
    pub records_deleted_before: Option<DateTime<Utc>>,
    pub total_deleted_records: u64,
    pub last_swept_at: Option<DateTime<Utc>>,
    pub last_completed_at: Option<DateTime<Utc>>,
    pub next_scan_at: DateTime<Utc>,
    pub version: u64,
}

impl From<AuditRetentionStatus> for AuditRetentionStatusResponse {
    fn from(status: AuditRetentionStatus) -> Self {
        Self {
            organization_id: status.organization_id.as_uuid(),
            retention_ms: status.retention_ms,
            policy_digest: status.policy_digest.to_string(),
            applied_policy_digest: status
                .applied_policy_digest
                .map(|digest| digest.to_string()),
            current_policy_applied: status.current_policy_applied,
            records_available_from: status.records_available_from,
            records_deleted_before: status.records_deleted_before,
            total_deleted_records: status.total_deleted_records,
            last_swept_at: status.last_swept_at,
            last_completed_at: status.last_completed_at,
            next_scan_at: status.next_scan_at,
            version: status.version,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::audit::AuditAttributionStatus;
    use crate::modules::shared_kernel::domain::{
        OrganizationId, PrincipalId, ProjectAttributionProfileId, ProjectId,
    };

    #[test]
    fn public_projection_never_exposes_unstructured_details() {
        let response = AuditRecordResponse::from(AuditRecord {
            id: Uuid::now_v7(),
            organization_id: OrganizationId::new(),
            actor_principal_id: Some(PrincipalId::new()),
            action: "identity.membership.created".into(),
            aggregate_id: Uuid::now_v7(),
            occurred_at: Utc::now(),
            request_id: Uuid::now_v7(),
            project_id: Some(ProjectId::new()),
            environment_id: None,
            attribution_profile_id: Some(ProjectAttributionProfileId::new()),
            attribution_status: AuditAttributionStatus::ProfileBound,
        });
        let value = serde_json::to_value(response).expect("audit response");
        assert_eq!(value.as_object().map(serde_json::Map::len), Some(11));
        assert!(!value.as_object().expect("object").contains_key("details"));
        for private in ["labels", "businessOwnerReference", "costAttributionCode"] {
            assert!(!value.as_object().expect("object").contains_key(private));
        }
    }
}
