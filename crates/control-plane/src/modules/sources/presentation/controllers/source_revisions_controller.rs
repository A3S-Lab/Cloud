use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
use crate::modules::sources::application::commands::resolve_external_source_revision::{
    DockerfileBuildRecipeInput, ResolveExternalSourceRevision,
};
use crate::modules::sources::presentation::dto::{
    ResolveSourceRevisionRequest, SourceRevisionResponse,
};
use crate::presentation::{OrganizationTenantGuard, application_error_response};
use a3s_boot::{
    controller, metadata, post, use_guard, AUTH_SCOPES_METADATA, BootError, BootRequest,
    BootResponse, CommandBus, ControllerDefinition, Result,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

pub fn source_revisions_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    Arc::new(SourceRevisionsController { bus }).controller()
}

#[derive(Debug, Clone)]
struct SourceRevisionsController {
    bus: Arc<CommandBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::SOURCE_WRITE])]
impl SourceRevisionsController {
    #[post(
        "/{organization_id}/projects/{project_id}/environments/{environment_id}/source-revisions",
        raw
    )]
    async fn resolve(&self, request: BootRequest) -> Result<BootResponse> {
        let body: ResolveSourceRevisionRequest = request.json_with_content_type()?;
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let project_id = ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?);
        let environment_id =
            EnvironmentId::from_uuid(request.param_as::<Uuid>("environment_id")?);
        let (idempotency_key, request_id) = request_identity(&request)?;
        match self
            .bus
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

#[cfg(test)]
mod nest_macro_source_revisions_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;

    #[test]
    fn source_revisions_controller_registers_scoped_guarded_post_via_nest_macros() {
        let controller = source_revisions_controller(Arc::new(CommandBus::new()))
            .expect("source revisions nest command controller");
        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].method(), HttpMethod::Post);
        assert_eq!(
            routes[0].path(),
            "/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/source-revisions"
        );
        assert_eq!(
            routes[0]
                .metadata()
                .get(AUTH_SCOPES_METADATA)
                .cloned()
                .expect("auth.scopes"),
            serde_json::json!([ApiTokenScope::SOURCE_WRITE])
        );
    }
}
