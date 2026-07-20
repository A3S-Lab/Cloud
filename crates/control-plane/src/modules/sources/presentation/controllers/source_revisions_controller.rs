use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::OrganizationTenantGuard;
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
use crate::modules::sources::application::commands::resolve_external_source_revision::{
    DockerfileBuildRecipeInput, ResolveExternalSourceRevision,
};
use crate::modules::sources::presentation::dto::{
    ResolveSourceRevisionRequest, SourceRevisionResponse,
};
use crate::presentation::application_error_response;
use a3s_boot::{
    BootError, BootRequest, BootResponse, CommandBus, ControllerDefinition, Result,
    AUTH_SCOPES_METADATA,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

pub fn source_revisions_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::SOURCE_WRITE])?
        .post(
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/source-revisions",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let body: ResolveSourceRevisionRequest = request.json_with_content_type()?;
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let project_id = ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?);
                    let environment_id =
                        EnvironmentId::from_uuid(request.param_as::<Uuid>("environment_id")?);
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(ResolveExternalSourceRevision {
                            organization_id,
                            project_id,
                            environment_id,
                            repository_provider: body.repository.provider,
                            repository_url: body.repository.url,
                            reference_kind: body.reference.kind,
                            reference_value: body.reference.value,
                            recipe: DockerfileBuildRecipeInput {
                                schema: body.recipe.schema,
                                kind: body.recipe.kind,
                                context_path: body.recipe.context_path,
                                dockerfile_path: body.recipe.dockerfile_path,
                                target: body.recipe.target,
                                platforms: body.recipe.platforms,
                            },
                            webhook_delivery_id: body.webhook_delivery_id,
                            idempotency_key,
                            request_id,
                            accepted_at: Utc::now(),
                        })
                        .await?
                    {
                        Ok(result) => {
                            let status = if result.replayed { 200 } else { 201 };
                            BootResponse::json_with_status(
                                status,
                                &SourceRevisionResponse::from_result(result),
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
