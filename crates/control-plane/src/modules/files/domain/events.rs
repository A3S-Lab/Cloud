use super::{UserFile, USER_FILE_ADMISSION_CONTRACT_SCHEMA};
use a3s_cloud_contracts::DomainEventEnvelope;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const USER_FILE_LIFECYCLE_EVENT_SCHEMA: &str = "cloud.user-file.lifecycle.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UserFileLifecycleChanged {
    pub schema: String,
    pub project_id: Uuid,
    pub user_file_id: Uuid,
    pub upload_id: Uuid,
    pub state: String,
    pub contract_schema: String,
    pub contract_digest: String,
    pub content_digest: String,
    pub size_bytes: u64,
    pub media_type: String,
    pub upload_expires_at: DateTime<Utc>,
    pub retention_until: DateTime<Utc>,
    pub cleanup_due_at: Option<DateTime<Utc>>,
    pub scan_evidence_digest: Option<String>,
    pub rejection_reason_code: Option<String>,
}

impl UserFileLifecycleChanged {
    pub fn changed(
        file: &UserFile,
        correlation_id: Uuid,
        causation_id: Option<Uuid>,
    ) -> Result<DomainEventEnvelope, String> {
        file.validate()?;
        if correlation_id.is_nil() || causation_id.is_some_and(|value| value.is_nil()) {
            return Err("UserFile event correlation or causation identity is invalid".into());
        }
        let content = &file.contract.spec().content;
        let payload = Self {
            schema: USER_FILE_LIFECYCLE_EVENT_SCHEMA.into(),
            project_id: file.project_id.as_uuid(),
            user_file_id: file.id.as_uuid(),
            upload_id: file.upload_id.as_uuid(),
            state: file.state.as_str().into(),
            contract_schema: USER_FILE_ADMISSION_CONTRACT_SCHEMA.into(),
            contract_digest: file.contract.digest().as_str().into(),
            content_digest: content.digest.as_str().into(),
            size_bytes: content.size_bytes,
            media_type: content.media_type.clone(),
            upload_expires_at: file.contract.spec().upload_expires_at,
            retention_until: file.contract.spec().retention_until,
            cleanup_due_at: file.cleanup_due_at(),
            scan_evidence_digest: file
                .scan_evidence_digest
                .as_ref()
                .map(|value| value.as_str().to_owned()),
            rejection_reason_code: file.rejection_reason_code.clone(),
        };
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "user-file.lifecycle.changed".into(),
            schema_version: 1,
            scope: a3s_cloud_contracts::CloudScopeRef::Organization {
                organization_id: file.organization_id.as_uuid(),
            },
            aggregate_id: file.id.as_uuid(),
            aggregate_version: file.aggregate_version,
            occurred_at: file.updated_at,
            correlation_id,
            causation_id,
            payload: serde_json::to_value(payload)
                .map_err(|error| format!("serialize UserFile lifecycle event: {error}"))?,
        })
    }
}
