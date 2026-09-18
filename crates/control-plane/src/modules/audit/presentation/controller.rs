use super::dto::{
    AuditExportManifestBundleResponse, AuditExportResponse, AuditRecordPageResponse,
    AuditRetentionStatusResponse,
};
use crate::modules::audit::application::{
    ExportAuditManifest, ExportAuditRecords, GetAuditRetentionStatus, ListAuditRecords,
    DEFAULT_AUDIT_EXPORT_MANIFEST_PAGE_SIZE, DEFAULT_AUDIT_RECORD_LIMIT,
    MAXIMUM_AUDIT_RECORD_LIMIT,
};
use crate::modules::audit::domain::{AuditAttributionStatus, AuditRecordFilter};
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, PrincipalId, ProjectAttributionProfileId, ProjectId,
};
use crate::presentation::{OrganizationAdministratorGuard, OrganizationTenantGuard, application_error_response};
use a3s_boot::{
    controller, get, metadata, use_guard, BootError, BootRequest, BootResponse,
    ControllerDefinition, QueryBus, Result, AUTH_SCOPES_METADATA,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

pub fn audit_query_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    Arc::new(AuditQueryController { bus }).controller()
}

#[derive(Debug, Clone)]
struct AuditQueryController {
    bus: Arc<QueryBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[use_guard(OrganizationAdministratorGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::CLOUD_READ])]
impl AuditQueryController {
    #[get("/{organization_id}/audit-records/export/manifest", raw)]
    async fn export_manifest(&self, request: BootRequest) -> Result<BootResponse> {
        let parameters: AuditExportManifestParameters = request.query()?;
        parameters.validate_page_size()?;
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(ExportAuditManifest {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                filter: parameters.filter(),
                page_size: parameters.page_size,
            })
            .await?
        {
            Ok(bundle) => BootResponse::json(&AuditExportManifestBundleResponse::from(bundle)),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get("/{organization_id}/audit-records/export", raw)]
    async fn export_records(&self, request: BootRequest) -> Result<BootResponse> {
        let parameters: AuditRecordParameters = request.query()?;
        parameters.validate_limit()?;
        let request_id = request_id(&request)?;
        match self
            .bus
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

    #[get("/{organization_id}/audit-records/retention", raw)]
    async fn retention(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(GetAuditRetentionStatus {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
            })
            .await?
        {
            Ok(status) => BootResponse::json(&AuditRetentionStatusResponse::from(status)),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get("/{organization_id}/audit-records", raw)]
    async fn list_records(&self, request: BootRequest) -> Result<BootResponse> {
        let parameters: AuditRecordParameters = request.query()?;
        parameters.validate_limit()?;
        let request_id = request_id(&request)?;
        match self
            .bus
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
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AuditExportManifestParameters {
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
    #[serde(default = "default_manifest_page_size")]
    page_size: usize,
}

impl AuditExportManifestParameters {
    fn validate_page_size(&self) -> Result<()> {
        if self.page_size == 0 || self.page_size > MAXIMUM_AUDIT_RECORD_LIMIT {
            return Err(BootError::BadRequest(format!(
                "audit export manifest page size must be between 1 and {MAXIMUM_AUDIT_RECORD_LIMIT}"
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

const fn default_manifest_page_size() -> usize {
    DEFAULT_AUDIT_EXPORT_MANIFEST_PAGE_SIZE
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

#[cfg(test)]
mod nest_macro_audit_query_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;
    use std::collections::HashSet;

    #[test]
    fn audit_queries_register_admin_scoped_gets_via_nest_macros() {
        let controller =
            audit_query_controller(Arc::new(QueryBus::new())).expect("audit nest queries");
        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 4);
        assert!(routes.iter().all(|route| route.method() == HttpMethod::Get));
        let paths: HashSet<_> = routes.iter().map(|route| route.path().to_string()).collect();
        assert_eq!(
            paths,
            HashSet::from([
                "/organizations/{organization_id}/audit-records".to_string(),
                "/organizations/{organization_id}/audit-records/export".to_string(),
                "/organizations/{organization_id}/audit-records/export/manifest".to_string(),
                "/organizations/{organization_id}/audit-records/retention".to_string(),
            ])
        );
        assert_eq!(
            routes[0]
                .metadata()
                .get(AUTH_SCOPES_METADATA)
                .cloned()
                .expect("auth.scopes"),
            serde_json::json!([ApiTokenScope::CLOUD_READ])
        );
    }
}
