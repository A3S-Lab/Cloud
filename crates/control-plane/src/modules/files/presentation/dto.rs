use crate::modules::files::application::UserFileMutationResult;
use crate::modules::files::domain::{UserFile, UserFileQuota, USER_FILE_ADMISSION_CONTRACT_SCHEMA};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReserveUserFileRequest {
    pub admission_acl: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TombstoneUserFileRequest {
    pub expected_version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserFileResponse {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub user_file_id: Uuid,
    pub upload_id: Uuid,
    pub state: String,
    pub original_name: String,
    pub contract_schema: String,
    pub admission_acl: String,
    pub contract_digest: String,
    pub object_ref: String,
    pub content_digest: String,
    pub size_bytes: u64,
    pub media_type: String,
    pub scan_policy: String,
    pub upload_expires_at: DateTime<Utc>,
    pub retention_until: DateTime<Utc>,
    pub scan_evidence_digest: Option<String>,
    pub rejection_reason_code: Option<String>,
    pub tombstoned_from: Option<String>,
    pub aggregate_version: u64,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub uploaded_at: Option<DateTime<Utc>>,
    pub scanned_at: Option<DateTime<Utc>>,
    pub expired_at: Option<DateTime<Utc>>,
    pub tombstoned_at: Option<DateTime<Utc>>,
    pub cleanup_due_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
}

impl From<UserFile> for UserFileResponse {
    fn from(file: UserFile) -> Self {
        let spec = file.contract.spec().clone();
        let admission_acl = file.contract.canonical_acl().to_owned();
        let contract_digest = file.contract.digest().as_str().to_owned();
        Self {
            organization_id: file.organization_id.as_uuid(),
            project_id: file.project_id.as_uuid(),
            user_file_id: file.id.as_uuid(),
            upload_id: file.upload_id.as_uuid(),
            state: file.state.as_str().into(),
            original_name: spec.original_name,
            contract_schema: USER_FILE_ADMISSION_CONTRACT_SCHEMA.into(),
            admission_acl,
            contract_digest,
            object_ref: spec.content.object_ref,
            content_digest: spec.content.digest.as_str().into(),
            size_bytes: spec.content.size_bytes,
            media_type: spec.content.media_type,
            scan_policy: spec.scan_policy.as_str().into(),
            upload_expires_at: spec.upload_expires_at,
            retention_until: spec.retention_until,
            scan_evidence_digest: file
                .scan_evidence_digest
                .as_ref()
                .map(|value| value.as_str().into()),
            rejection_reason_code: file.rejection_reason_code.clone(),
            tombstoned_from: file.tombstoned_from.map(|state| state.as_str().into()),
            aggregate_version: file.aggregate_version,
            created_by: file.created_by.as_uuid(),
            created_at: file.created_at,
            uploaded_at: file.uploaded_at,
            scanned_at: file.scanned_at,
            expired_at: file.expired_at,
            tombstoned_at: file.tombstoned_at,
            cleanup_due_at: file.cleanup_due_at(),
            updated_at: file.updated_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserFileMutationResponse {
    pub file: UserFileResponse,
    pub replayed: bool,
}

impl From<UserFileMutationResult> for UserFileMutationResponse {
    fn from(result: UserFileMutationResult) -> Self {
        Self {
            file: result.file.into(),
            replayed: result.replayed,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserFileQuotaResponse {
    pub organization_id: Uuid,
    pub limit_bytes: u64,
    pub allocated_bytes: u64,
    pub available_bytes: u64,
    pub revision: u64,
    pub updated_at: Option<DateTime<Utc>>,
}

impl From<UserFileQuota> for UserFileQuotaResponse {
    fn from(quota: UserFileQuota) -> Self {
        Self {
            organization_id: quota.organization_id.as_uuid(),
            limit_bytes: quota.limit_bytes,
            allocated_bytes: quota.allocated_bytes,
            available_bytes: quota.available_bytes(),
            revision: quota.revision,
            updated_at: quota.updated_at,
        }
    }
}
