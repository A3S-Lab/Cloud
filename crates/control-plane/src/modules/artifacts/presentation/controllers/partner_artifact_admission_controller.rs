use crate::modules::artifacts::domain::entities::PartnerArtifactAdmission;
use crate::modules::artifacts::{
    AdmitPartnerArtifact, GetPartnerArtifactAdmission, ListPartnerArtifactAdmissions,
};
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::{
    OrganizationAdministratorGuard, OrganizationTenantGuard,
};
use crate::modules::shared_kernel::domain::OrganizationId;
use crate::presentation::{application_error_response, request_identity};
use a3s_boot::{
    controller, get, metadata, post, use_guard, BootError, BootRequest, BootResponse, CommandBus,
    ControllerDefinition, QueryBus, Result, AUTH_SCOPES_METADATA,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AdmitPartnerArtifactRequest {
    pub content_digest: String,
    pub kind: String,
    pub byte_size: u64,
    pub partner_ref: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PartnerArtifactAdmissionResponse {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub content_digest: String,
    pub kind: String,
    pub byte_size: u64,
    pub partner_ref: String,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replayed: Option<bool>,
}

impl PartnerArtifactAdmissionResponse {
    fn from_admission(admission: PartnerArtifactAdmission, replayed: Option<bool>) -> Self {
        Self {
            id: admission.id.as_uuid(),
            organization_id: admission.organization_id.as_uuid(),
            content_digest: admission.content_digest.as_str().to_owned(),
            kind: admission.kind.as_str().to_owned(),
            byte_size: admission.byte_size,
            partner_ref: admission.partner_ref,
            aggregate_version: admission.aggregate_version,
            created_at: admission.created_at,
            replayed,
        }
    }
}

pub fn partner_artifact_admission_controller(
    command_bus: Arc<CommandBus>,
    query_bus: Arc<QueryBus>,
) -> Result<ControllerDefinition> {
    Arc::new(PartnerArtifactAdmissionController {
        command_bus,
        query_bus,
    })
    .controller()
}

#[derive(Debug, Clone)]
struct PartnerArtifactAdmissionController {
    command_bus: Arc<CommandBus>,
    query_bus: Arc<QueryBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[use_guard(OrganizationAdministratorGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::BUILD_WRITE])]
impl PartnerArtifactAdmissionController {
    #[post("/{organization_id}/partner-artifact-admissions", raw)]
    async fn admit(&self, request: BootRequest) -> Result<BootResponse> {
        let body: AdmitPartnerArtifactRequest = request.json_with_content_type()?;
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let (idempotency_key, request_id) = request_identity(&request)?;
        match self
            .command_bus
            .execute(AdmitPartnerArtifact {
                organization_id,
                content_digest: body.content_digest,
                kind: body.kind,
                byte_size: body.byte_size,
                partner_ref: body.partner_ref,
                idempotency_key,
                request_id,
            })
            .await?
        {
            Ok(result) => {
                let status = if result.replayed { 200 } else { 201 };
                BootResponse::json_with_status(
                    status,
                    &PartnerArtifactAdmissionResponse::from_admission(
                        result.admission,
                        Some(result.replayed),
                    ),
                )
            }
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get("/{organization_id}/partner-artifact-admissions", raw)]
    async fn list(&self, request: BootRequest) -> Result<BootResponse> {
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let request_id = request
            .header("x-request-id")
            .and_then(|value| Uuid::parse_str(value).ok())
            .unwrap_or_else(Uuid::now_v7);
        match self
            .query_bus
            .execute(ListPartnerArtifactAdmissions { organization_id })
            .await?
        {
            Ok(items) => BootResponse::json(
                &items
                    .into_iter()
                    .map(|item| PartnerArtifactAdmissionResponse::from_admission(item, None))
                    .collect::<Vec<_>>(),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get("/{organization_id}/partner-artifact-admissions/{admission_id}", raw)]
    async fn get(&self, request: BootRequest) -> Result<BootResponse> {
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let admission_id = request.param_as::<Uuid>("admission_id")?;
        let request_id = request
            .header("x-request-id")
            .and_then(|value| Uuid::parse_str(value).ok())
            .unwrap_or_else(Uuid::now_v7);
        match self
            .query_bus
            .execute(GetPartnerArtifactAdmission {
                organization_id,
                admission_id,
            })
            .await?
        {
            Ok(admission) => BootResponse::json(&PartnerArtifactAdmissionResponse::from_admission(
                admission, None,
            )),
            Err(error) => application_error_response(error, request_id),
        }
    }
}

#[allow(dead_code)]
fn _boot_error_marker() -> BootError {
    BootError::BadRequest("unused".into())
}
