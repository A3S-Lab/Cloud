use super::tool_result;
use crate::modules::audit::presentation::AuditRecordPageResponse;
use crate::modules::audit::{AuditRecordFilter, ListAuditRecords};
use crate::modules::shared_kernel::domain::{OrganizationId, PrincipalId};
use a3s_boot::{QueryBus, Result};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuditRecordListArguments {
    actor_principal_id: Option<Uuid>,
    action: Option<String>,
    aggregate_id: Option<Uuid>,
    request_id: Option<Uuid>,
    from: Option<DateTime<Utc>>,
    to: Option<DateTime<Utc>>,
    cursor: Option<String>,
    #[serde(
        default = "super::arguments::default_list_limit",
        deserialize_with = "super::arguments::deserialize_list_limit"
    )]
    limit: usize,
}

pub async fn list_audit_records(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: AuditRecordListArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ListAuditRecords {
            organization_id,
            filter: AuditRecordFilter {
                actor_principal_id: arguments.actor_principal_id.map(PrincipalId::from_uuid),
                action: arguments.action,
                aggregate_id: arguments.aggregate_id,
                request_id: arguments.request_id,
                from: arguments.from,
                to: arguments.to,
            },
            cursor: arguments.cursor,
            limit: arguments.limit,
        })
        .await?
    {
        Ok(page) => tool_result::success(200, AuditRecordPageResponse::from(page), request_id),
        Err(error) => tool_result::application_error(error, request_id),
    }
}
