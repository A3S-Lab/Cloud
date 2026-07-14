use crate::modules::identity::domain::entities::ApiToken;
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::OrganizationId;
use a3s_boot::Command;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::BTreeSet;
use uuid::Uuid;

#[derive(Clone)]
pub struct CreateApiToken {
    pub organization_id: OrganizationId,
    pub name: String,
    pub token_secret: String,
    pub scopes: Vec<String>,
    pub issuer_scopes: BTreeSet<ApiTokenScope>,
    pub expires_at: Option<DateTime<Utc>>,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for CreateApiToken {
    type Output = ApplicationResult<CreateApiTokenResult>;
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateApiTokenResult {
    pub api_token: ApiToken,
    pub replayed: bool,
}
