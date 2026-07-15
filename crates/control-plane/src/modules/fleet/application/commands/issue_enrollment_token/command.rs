use crate::modules::fleet::domain::entities::EnrollmentToken;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::OrganizationId;
use a3s_boot::Command;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Clone)]
pub struct IssueEnrollmentToken {
    pub organization_id: OrganizationId,
    pub name: String,
    pub token_secret: String,
    pub expires_at: DateTime<Utc>,
    pub idempotency_key: String,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

impl Command for IssueEnrollmentToken {
    type Output = ApplicationResult<IssueEnrollmentTokenResult>;
}

#[derive(Debug, Clone, Serialize)]
pub struct IssueEnrollmentTokenResult {
    pub enrollment_token: EnrollmentToken,
    pub replayed: bool,
}
