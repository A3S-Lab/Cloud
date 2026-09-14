//! Applications management delivery for asynchronous invocation observation.
//!
//! `APP0.2-C47` exposes the C46 project-authorized CQRS poll through REST only.
//! No streaming cursor, SSE, Gateway route, or public availability is added.

use super::delivery_dto::ApplicationAsynchronousObservationResponse;
use crate::access_projection::application_access;
use crate::modules::applications::application::ObserveApplicationAsynchronousInvocation;
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::{resource_access_evaluator, OrganizationTenantGuard};
use crate::modules::shared_kernel::domain::{
    ApplicationId, ApplicationInvocationId, ApplicationSessionId, OrganizationId, ProjectId,
};
use crate::presentation::{actor_principal_id, application_error_response, request_id};
use a3s_boot::{
    BootRequest, BootResponse, ControllerDefinition, QueryBus, Result, AUTH_SCOPES_METADATA,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

pub fn application_asynchronous_observation_queries_controller(
    bus: Arc<QueryBus>,
) -> Result<ControllerDefinition> {
    let observe_bus = Arc::clone(&bus);
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::APPLICATION_WRITE])?
        .get(
            "/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/invocations/{invocation_id}/asynchronous-observation",
            move |request: BootRequest| {
                let bus = Arc::clone(&observe_bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(ObserveApplicationAsynchronousInvocation {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            project_id: ProjectId::from_uuid(
                                request.param_as::<Uuid>("project_id")?,
                            ),
                            application_id: ApplicationId::from_uuid(
                                request.param_as::<Uuid>("application_id")?,
                            ),
                            session_id: ApplicationSessionId::from_uuid(
                                request.param_as::<Uuid>("session_id")?,
                            ),
                            invocation_id: ApplicationInvocationId::from_uuid(
                                request.param_as::<Uuid>("invocation_id")?,
                            ),
                            actor_principal_id: actor_principal_id(&request)?,
                            access: application_access(&resource_access_evaluator(
                                &request.require_auth_principal()?,
                            )?),
                            observed_at: Utc::now(),
                        })
                        .await?
                    {
                        Ok(observation) => BootResponse::json(
                            &ApplicationAsynchronousObservationResponse::from(observation),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )
}
