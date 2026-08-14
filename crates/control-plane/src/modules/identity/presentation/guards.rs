use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::identity::domain::value_objects::BootstrapCredential;
use crate::modules::identity::domain::value_objects::ResourceGrantScope;
use crate::modules::identity::presentation::resource_access_evaluator;
use crate::modules::shared_kernel::domain::{EnvironmentId, NodeId, ProjectId};
use a3s_boot::{
    BootError, BootRequest, BoxFuture, ExecutionContext, Guard, HttpMethod, Result, RouteDefinition,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const DEFERRED_RESOURCE_SCOPE_METADATA: &str = "a3s.cloud.resourceAccess.deferredScope";

/// Declares which resource family an application handler will resolve from an indirect ID.
///
/// The tenant guard uses this only for coarse admission. The owning application module must load
/// the resource, derive its canonical grant scope, and make the final authorization decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeferredResourceScope {
    Project,
    Node,
    Any,
    /// The owning handler resolves a record addressed to the authenticated Principal itself.
    Personal,
}

pub fn with_deferred_resource_scope(
    route: RouteDefinition,
    scope: DeferredResourceScope,
) -> Result<RouteDefinition> {
    route.with_metadata(DEFERRED_RESOURCE_SCOPE_METADATA, scope)
}

#[derive(Clone)]
pub struct BootstrapGuard {
    credential: BootstrapCredential,
}

impl BootstrapGuard {
    pub fn new(credential: BootstrapCredential) -> Self {
        Self { credential }
    }
}

impl Guard for BootstrapGuard {
    fn can_activate(&self, context: ExecutionContext) -> BoxFuture<'static, Result<bool>> {
        let credential = self.credential.clone();
        Box::pin(async move {
            let candidate = context
                .request
                .header("x-a3s-bootstrap-token")
                .ok_or_else(|| {
                    BootError::Unauthorized("missing bootstrap credential".to_string())
                })?;
            if !credential.verify(candidate) {
                return Err(BootError::Unauthorized(
                    "invalid bootstrap credential".to_string(),
                ));
            }
            Ok(true)
        })
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct OrganizationTenantGuard;

impl Guard for OrganizationTenantGuard {
    fn can_activate(&self, context: ExecutionContext) -> BoxFuture<'static, Result<bool>> {
        Box::pin(async move {
            let Some(requested) = context.request.param("organization_id") else {
                return Ok(true);
            };
            let principal = context.request.require_auth_principal()?;
            if principal.has_role("platform_admin") {
                return Ok(true);
            }
            let authenticated = principal
                .claim("organization_id")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| {
                    BootError::Forbidden(
                        "authenticated principal has no organization context".to_string(),
                    )
                })?;
            if requested != authenticated {
                return Err(BootError::Forbidden(
                    "authenticated token cannot access another organization".to_string(),
                ));
            }
            let evaluator = resource_access_evaluator(&principal)?;
            if evaluator.is_organization_wide() {
                return Ok(true);
            }
            if collection_is_visible(&context.request, &context.route_path, &evaluator)? {
                return Ok(true);
            }
            if let Some(scope) =
                context.metadata_as::<DeferredResourceScope>(DEFERRED_RESOURCE_SCOPE_METADATA)?
            {
                if deferred_resource_is_visible(scope, &evaluator) {
                    return Ok(true);
                }
                return Err(missing_resource_grant());
            }
            let resource = resource_scope(&context.request)?;
            if resource.is_some_and(|resource| evaluator.allows(resource)) {
                return Ok(true);
            }
            Err(missing_resource_grant())
        })
    }
}

fn deferred_resource_is_visible(
    scope: DeferredResourceScope,
    evaluator: &ResourceAccessEvaluator,
) -> bool {
    match scope {
        DeferredResourceScope::Project => evaluator.has_project_visibility(),
        DeferredResourceScope::Node => evaluator.has_node_visibility(),
        DeferredResourceScope::Any => evaluator.has_any_visible_resource(),
        DeferredResourceScope::Personal => true,
    }
}

fn missing_resource_grant() -> BootError {
    BootError::Forbidden(
        "restricted membership has no grant for the requested resource".to_string(),
    )
}

fn collection_is_visible(
    request: &BootRequest,
    route_path: &str,
    evaluator: &ResourceAccessEvaluator,
) -> Result<bool> {
    if request.method() != HttpMethod::Get {
        return Ok(false);
    }
    if route_path.ends_with("/organizations/{organization_id}/projects") {
        return Ok(evaluator.has_project_visibility());
    }
    if route_path.ends_with("/organizations/{organization_id}/nodes") {
        return Ok(evaluator.has_node_visibility());
    }
    if route_path.ends_with("/organizations/{organization_id}/search") {
        return Ok(evaluator.has_any_visible_resource());
    }
    if route_path.ends_with("/organizations/{organization_id}/projects/{project_id}/environments") {
        let Some(project_id) = request.param("project_id") else {
            return Ok(false);
        };
        return Ok(evaluator
            .project_is_visible_in_collection(ProjectId::from_uuid(parse_uuid(project_id)?)));
    }
    Ok(false)
}

fn resource_scope(request: &BootRequest) -> Result<Option<ResourceGrantScope>> {
    let project_id = request
        .param("project_id")
        .map(parse_uuid)
        .transpose()?
        .map(ProjectId::from_uuid);
    let environment_id = request
        .param("environment_id")
        .map(parse_uuid)
        .transpose()?
        .map(EnvironmentId::from_uuid);
    let node_id = request
        .param("node_id")
        .map(parse_uuid)
        .transpose()?
        .map(NodeId::from_uuid);
    match (project_id, environment_id, node_id) {
        (Some(project_id), Some(environment_id), None) => {
            Ok(Some(ResourceGrantScope::Environment {
                project_id,
                environment_id,
            }))
        }
        (Some(project_id), None, None) => Ok(Some(ResourceGrantScope::Project { project_id })),
        (None, None, Some(node_id)) => Ok(Some(ResourceGrantScope::Node { node_id })),
        (None, None, None) => Ok(None),
        _ => Err(BootError::Internal(
            "resource-scoped route parameters are inconsistent".into(),
        )),
    }
}

fn parse_uuid(value: &str) -> Result<Uuid> {
    Uuid::parse_str(value)
        .map_err(|error| BootError::BadRequest(format!("invalid resource identifier: {error}")))
}

#[derive(Debug, Clone, Copy, Default)]
pub struct OrganizationAdministratorGuard;

impl Guard for OrganizationAdministratorGuard {
    fn can_activate(&self, context: ExecutionContext) -> BoxFuture<'static, Result<bool>> {
        Box::pin(async move {
            let principal = context.request.require_auth_principal()?;
            if principal.has_role("platform_admin")
                || principal.has_role("organization_owner")
                || principal.has_role("organization_admin")
            {
                return Ok(true);
            }
            Err(BootError::Forbidden(
                "organization membership administration requires owner or admin role".into(),
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restricted_collection_visibility_uses_closed_read_only_route_shapes() {
        let project_id = ProjectId::new();
        let evaluator =
            ResourceAccessEvaluator::restricted([ResourceGrantScope::Project { project_id }]);
        let projects = BootRequest::new(
            HttpMethod::Get,
            format!("/api/v1/organizations/{}/projects", Uuid::now_v7()),
        );
        assert!(collection_is_visible(
            &projects,
            "/api/v1/organizations/{organization_id}/projects",
            &evaluator,
        )
        .expect("project collection visibility"));
        assert!(!collection_is_visible(
            &projects,
            "/api/v1/organizations/{organization_id}/plugin-registries/{registry_id}/search",
            &evaluator,
        )
        .expect("nested search visibility"));

        let mutation = BootRequest::new(HttpMethod::Post, projects.path());
        assert!(!collection_is_visible(
            &mutation,
            "/api/v1/organizations/{organization_id}/projects",
            &evaluator,
        )
        .expect("project mutation visibility"));

        let environments = BootRequest::new(
            HttpMethod::Get,
            format!(
                "/api/v1/organizations/{}/projects/{}/environments",
                Uuid::now_v7(),
                project_id.as_uuid()
            ),
        )
        .with_param("project_id", project_id.to_string());
        assert!(collection_is_visible(
            &environments,
            "/api/v1/organizations/{organization_id}/projects/{project_id}/environments",
            &evaluator,
        )
        .expect("environment collection visibility"));
    }

    #[test]
    fn deferred_scope_only_admits_a_matching_resource_family() {
        let project_id = ProjectId::new();
        let project =
            ResourceAccessEvaluator::restricted([ResourceGrantScope::Project { project_id }]);
        assert!(deferred_resource_is_visible(
            DeferredResourceScope::Project,
            &project
        ));
        assert!(!deferred_resource_is_visible(
            DeferredResourceScope::Node,
            &project
        ));

        let node = ResourceAccessEvaluator::restricted([ResourceGrantScope::Node {
            node_id: NodeId::new(),
        }]);
        assert!(!deferred_resource_is_visible(
            DeferredResourceScope::Project,
            &node
        ));
        assert!(deferred_resource_is_visible(
            DeferredResourceScope::Any,
            &node
        ));
    }

    #[test]
    fn deferred_scope_route_metadata_is_typed_and_explicit() {
        let route = with_deferred_resource_scope(
            RouteDefinition::get("/workloads/{workload_id}", |_request: BootRequest| async {
                Ok(a3s_boot::BootResponse::default())
            })
            .expect("route"),
            DeferredResourceScope::Project,
        )
        .expect("deferred scope metadata");
        assert_eq!(
            route
                .metadata_as::<DeferredResourceScope>(DEFERRED_RESOURCE_SCOPE_METADATA)
                .expect("typed metadata"),
            Some(DeferredResourceScope::Project)
        );
    }
}
