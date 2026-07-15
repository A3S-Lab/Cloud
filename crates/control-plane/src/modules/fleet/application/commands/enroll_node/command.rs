use crate::modules::shared_kernel::application::ApplicationResult;
use a3s_boot::Command;
use a3s_cloud_contracts::{NodeEnrollmentRequest, NodeEnrollmentResponse};
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Clone)]
pub struct EnrollNode {
    pub request: NodeEnrollmentRequest,
    pub request_id: Uuid,
    pub received_at: DateTime<Utc>,
}

impl Command for EnrollNode {
    type Output = ApplicationResult<EnrollNodeResult>;
}

#[derive(Debug, Clone)]
pub struct EnrollNodeResult {
    pub response: NodeEnrollmentResponse,
    pub replayed: bool,
}
