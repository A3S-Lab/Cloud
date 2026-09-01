use crate::modules::artifacts::application::{CancelBuildRun, RetryBuildRun};
use crate::modules::artifacts::presentation::dto::{CancelBuildRunResponse, RetryBuildRunResponse};
use crate::modules::shared_kernel::domain::{BuildRunId, OrganizationId};
use crate::presentation::{
    application_error_response, artifact_access, organization_tenant_build_write_controller,
    request_identity, resource_access_evaluator, with_deferred_resource_scope,
    DeferredResourceScope,
};
use a3s_boot::{
    BootRequest, BootResponse, CommandBus, ControllerDefinition, Result, RouteDefinition,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

pub fn build_run_commands_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    let cancel_bus = Arc::clone(&bus);
    let controller = ControllerDefinition::new("/organizations")?
        .route(with_deferred_resource_scope(
            RouteDefinition::delete(
                "/{organization_id}/build-runs/{build_run_id}",
                move |request: BootRequest| {
                    let bus = Arc::clone(&cancel_bus);
                    async move {
                        let organization_id =
                            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                        let build_run_id =
                            BuildRunId::from_uuid(request.param_as::<Uuid>("build_run_id")?);
                        let access = artifact_access(&resource_access_evaluator(
                            &request.require_auth_principal()?,
                        )?);
                        let (idempotency_key, request_id) = request_identity(&request)?;
                        match bus
                            .execute(CancelBuildRun {
                                organization_id,
                                build_run_id,
                                access,
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
            )?,
            DeferredResourceScope::Project,
        )?)?
        .route(with_deferred_resource_scope(
            RouteDefinition::post(
                "/{organization_id}/build-runs/{build_run_id}/retry",
                move |request: BootRequest| {
                    let bus = Arc::clone(&bus);
                    async move {
                        let organization_id =
                            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                        let build_run_id =
                            BuildRunId::from_uuid(request.param_as::<Uuid>("build_run_id")?);
                        let access = artifact_access(&resource_access_evaluator(
                            &request.require_auth_principal()?,
                        )?);
                        let (idempotency_key, request_id) = request_identity(&request)?;
                        match bus
                            .execute(RetryBuildRun {
                                organization_id,
                                build_run_id,
                                access,
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
            )?,
            DeferredResourceScope::Project,
        )?)?;
    organization_tenant_build_write_controller(controller)
}
