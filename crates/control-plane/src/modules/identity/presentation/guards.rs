use crate::modules::identity::domain::value_objects::BootstrapCredential;
use crate::modules::identity::domain::value_objects::ResourceGrantScope;
use crate::modules::identity::presentation::resource_access_evaluator;
use crate::modules::shared_kernel::domain::{EnvironmentId, NodeId, ProjectId};
use a3s_boot::{BootError, BoxFuture, ExecutionContext, Guard, Result};
use uuid::Uuid;

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
            let resource = resource_scope(&context.request)?;
            if resource.is_some_and(|resource| evaluator.allows(resource)) {
                return Ok(true);
            }
            Err(BootError::Forbidden(
                "restricted membership has no grant for the requested resource".to_string(),
            ))
        })
    }
}

fn resource_scope(request: &a3s_boot::BootRequest) -> Result<Option<ResourceGrantScope>> {
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
