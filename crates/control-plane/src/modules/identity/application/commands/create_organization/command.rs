use crate::modules::identity::domain::entities::Organization;
use crate::modules::shared_kernel::application::ApplicationResult;
use a3s_boot::Command;
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CreateOrganization {
    pub name: String,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for CreateOrganization {
    type Output = ApplicationResult<CreateOrganizationResult>;
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateOrganizationResult {
    pub organization: Organization,
    pub replayed: bool,
}
