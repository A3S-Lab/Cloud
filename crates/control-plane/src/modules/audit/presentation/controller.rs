use super::dto::AuditRecordPageResponse;
use crate::modules::audit::application::{
    ListAuditRecords, DEFAULT_AUDIT_RECORD_LIMIT, MAXIMUM_AUDIT_RECORD_LIMIT,
};
use crate::modules::audit::domain::AuditRecordFilter;
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::{
    OrganizationAdministratorGuard, OrganizationTenantGuard,
};
use crate::modules::shared_kernel::domain::{OrganizationId, PrincipalId};
use crate::presentation::application_error_response;
use a3s_boot::{
    BootError, BootRequest, BootResponse, ControllerDefinition, QueryBus, Result,
    AUTH_SCOPES_METADATA,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

pub fn audit_query_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .with_guard(OrganizationAdministratorGuard)
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::CLOUD_READ])?
        .get(
            "/{organization_id}/audit-records",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let parameters: AuditRecordParameters = request.query()?;
                    if parameters.limit == 0 || parameters.limit > MAXIMUM_AUDIT_RECORD_LIMIT {
                        return Err(BootError::BadRequest(format!(
                            "audit record limit must be between 1 and {MAXIMUM_AUDIT_RECORD_LIMIT}"
                        )));
                    }
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(ListAuditRecords {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            filter: AuditRecordFilter {
                                actor_principal_id: parameters
                                    .actor_principal_id
                                    .map(PrincipalId::from_uuid),
                                action: parameters.action,
                                aggregate_id: parameters.aggregate_id,
                                request_id: parameters.request_id,
                                from: parameters.from,
                                to: parameters.to,
                            },
                            cursor: parameters.cursor,
                            limit: parameters.limit,
                        })
                        .await?
                    {
                        Ok(page) => BootResponse::json(&AuditRecordPageResponse::from(page)),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AuditRecordParameters {
    #[serde(default)]
    actor_principal_id: Option<Uuid>,
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    aggregate_id: Option<Uuid>,
    #[serde(default)]
    request_id: Option<Uuid>,
    #[serde(default)]
    from: Option<DateTime<Utc>>,
    #[serde(default)]
    to: Option<DateTime<Utc>>,
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default = "default_limit")]
    limit: usize,
}

const fn default_limit() -> usize {
    DEFAULT_AUDIT_RECORD_LIMIT
}

fn request_id(request: &BootRequest) -> Result<Uuid> {
    request
        .header("x-request-id")
        .ok_or_else(|| BootError::Internal("request ID middleware did not run".into()))
        .and_then(|value| {
            Uuid::parse_str(value)
                .map_err(|error| BootError::Internal(format!("invalid request ID: {error}")))
        })
}
