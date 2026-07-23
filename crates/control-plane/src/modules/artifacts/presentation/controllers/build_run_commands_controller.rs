use crate::modules::artifacts::application::{CancelBuildRun, RetryBuildRun};
use crate::modules::artifacts::presentation::dto::{CancelBuildRunResponse, RetryBuildRunResponse};
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::OrganizationTenantGuard;
use crate::modules::shared_kernel::domain::{BuildRunId, OrganizationId};
use crate::presentation::application_error_response;
use a3s_boot::{
    BootError, BootRequest, BootResponse, CommandBus, ControllerDefinition, Result,
    AUTH_SCOPES_METADATA,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

pub fn build_run_commands_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    let cancel_bus = Arc::clone(&bus);
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::BUILD_WRITE])?
        .delete(
            "/{organization_id}/build-runs/{build_run_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&cancel_bus);
                async move {
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let build_run_id =
                        BuildRunId::from_uuid(request.param_as::<Uuid>("build_run_id")?);
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(CancelBuildRun {
                            organization_id,
                            build_run_id,
                            idempotency_key,
                            requested_at: Utc::now(),
                        })
                        .await?
                    {
                        Ok(result) => {
                            let status = if result.replayed { 200 } else { 202 };
                            BootResponse::json_with_status(
                                status,
                                &CancelBuildRunResponse::from(result),
                            )
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .post(
            "/{organization_id}/build-runs/{build_run_id}/retry",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let build_run_id =
                        BuildRunId::from_uuid(request.param_as::<Uuid>("build_run_id")?);
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(RetryBuildRun {
                            organization_id,
                            build_run_id,
                            idempotency_key,
                            requested_at: Utc::now(),
                        })
                        .await?
                    {
                        Ok(result) => {
                            let status = if result.replayed { 200 } else { 202 };
                            BootResponse::json_with_status(
                                status,
                                &RetryBuildRunResponse::from(result),
                            )
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )
}

fn request_identity(request: &BootRequest) -> Result<(String, Uuid)> {
    let idempotency_key = request
        .header("idempotency-key")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| BootError::BadRequest("idempotency-key header is required".into()))?
        .to_owned();
    let request_id = request
        .header("x-request-id")
        .ok_or_else(|| BootError::Internal("request ID middleware did not run".into()))
        .and_then(|value| {
            Uuid::parse_str(value)
                .map_err(|error| BootError::Internal(format!("invalid request ID: {error}")))
        })?;
    Ok((idempotency_key, request_id))
}
