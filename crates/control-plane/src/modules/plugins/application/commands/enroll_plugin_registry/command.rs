use crate::modules::plugins::domain::entities::PluginRegistry;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, PrincipalId};
use a3s_boot::Command;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Clone)]
pub struct EnrollPluginRegistry {
    pub organization_id: OrganizationId,
    pub actor_id: PrincipalId,
    pub name: String,
    pub endpoint: String,
    pub bootstrap_root: Vec<u8>,
    pub idempotency_key: String,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

impl Command for EnrollPluginRegistry {
    type Output = ApplicationResult<EnrollPluginRegistryResult>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EnrollPluginRegistryResult {
    pub registry: PluginRegistry,
    pub replayed: bool,
}
