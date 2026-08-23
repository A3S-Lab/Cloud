use super::dto::{AuditExportResponse, AuditRecordPageResponse};
use crate::modules::audit::application::{
    ExportAuditRecords, ListAuditRecords, DEFAULT_AUDIT_RECORD_LIMIT, MAXIMUM_AUDIT_RECORD_LIMIT,
};
use crate::modules::audit::domain::{AuditAttributionStatus, AuditRecordFilter};
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::{
    OrganizationAdministratorGuard, OrganizationTenantGuard,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, PrincipalId, ProjectAttributionProfileId, ProjectId,
};
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
    let export_bus = Arc::clone(&bus);
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .with_guard(OrganizationAdministratorGuard)
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::CLOUD_READ])?
        .get(
            "/{organization_id}/audit-records/export",
            move |request: BootRequest| {
                let bus = Arc::clone(&export_bus);
                async move {
                    let parameters: AuditRecordParameters = request.query()?;
                    parameters.validate_limit()?;
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(ExportAuditRecords {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            filter: parameters.filter(),
                            cursor: parameters.cursor,
                            limit: parameters.limit,
                        })
                        .await?
                    {
                        Ok(export) => BootResponse::json(&AuditExportResponse::from(export)),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .get(
            "/{organization_id}/audit-records",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let parameters: AuditRecordParameters = request.query()?;
                    parameters.validate_limit()?;
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(ListAuditRecords {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            filter: parameters.filter(),
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
    project_id: Option<Uuid>,
    #[serde(default)]
    environment_id: Option<Uuid>,
    #[serde(default)]
    attribution_profile_id: Option<Uuid>,
    #[serde(default)]
    attribution_status: Option<AuditAttributionStatus>,
    #[serde(default)]
    from: Option<DateTime<Utc>>,
    #[serde(default)]
    to: Option<DateTime<Utc>>,
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default = "default_limit")]
    limit: usize,
}

impl AuditRecordParameters {
    fn validate_limit(&self) -> Result<()> {
        if self.limit == 0 || self.limit > MAXIMUM_AUDIT_RECORD_LIMIT {
            return Err(BootError::BadRequest(format!(
                "audit record limit must be between 1 and {MAXIMUM_AUDIT_RECORD_LIMIT}"
            )));
        }
        Ok(())
    }

    fn filter(&self) -> AuditRecordFilter {
        AuditRecordFilter {
            actor_principal_id: self.actor_principal_id.map(PrincipalId::from_uuid),
            action: self.action.clone(),
            aggregate_id: self.aggregate_id,
            request_id: self.request_id,
            project_id: self.project_id.map(ProjectId::from_uuid),
            environment_id: self.environment_id.map(EnvironmentId::from_uuid),
            attribution_profile_id: self
                .attribution_profile_id
                .map(ProjectAttributionProfileId::from_uuid),
            attribution_status: self.attribution_status,
            from: self.from,
            to: self.to,
        }
    }
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
