use crate::modules::files::UserFileAccess;
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::identity::domain::value_objects::ResourceGrantScope;
use crate::modules::identity::presentation::authenticated_actor;
use crate::modules::search::{SearchVisibility, SearchVisibilityScope};
use crate::modules::shared_kernel::domain::PrincipalId;
use a3s_boot::{BootError, BootRequest, Result};
use uuid::Uuid;

pub(crate) fn request_identity(request: &BootRequest) -> Result<(String, Uuid)> {
    let idempotency_key = request
        .header("idempotency-key")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| BootError::BadRequest("idempotency-key header is required".into()))?
        .to_owned();
    Ok((idempotency_key, request_id(request)?))
}

pub(crate) fn actor_principal_id(request: &BootRequest) -> Result<PrincipalId> {
    let principal = request.require_auth_principal()?;
    Ok(authenticated_actor(&principal)?.principal_id)
}

pub(crate) fn search_visibility(resource_access: &ResourceAccessEvaluator) -> SearchVisibility {
    if resource_access.is_organization_wide() {
        return SearchVisibility::organization_wide();
    }
    SearchVisibility::restricted(resource_access.granted_scopes().map(|scope| match scope {
        ResourceGrantScope::Project { project_id } => SearchVisibilityScope::Project { project_id },
        ResourceGrantScope::Environment {
            project_id,
            environment_id,
        } => SearchVisibilityScope::Environment {
            project_id,
            environment_id,
        },
        ResourceGrantScope::Node { node_id } => SearchVisibilityScope::Node { node_id },
    }))
}

pub(crate) fn user_file_access(resource_access: &ResourceAccessEvaluator) -> UserFileAccess {
    if resource_access.is_organization_wide() {
        return UserFileAccess::organization_wide();
    }
    UserFileAccess::restricted_projects(resource_access.granted_scopes().filter_map(|scope| {
        match scope {
            ResourceGrantScope::Project { project_id } => Some(project_id),
            ResourceGrantScope::Environment { .. } | ResourceGrantScope::Node { .. } => None,
        }
    }))
}

pub(crate) fn request_id(request: &BootRequest) -> Result<Uuid> {
    request
        .header("x-request-id")
        .ok_or_else(|| BootError::Internal("request ID middleware did not run".into()))
        .and_then(|value| {
            Uuid::parse_str(value)
                .map_err(|error| BootError::Internal(format!("invalid request ID: {error}")))
        })
}

#[cfg(test)]
mod tests {
    use super::{search_visibility, user_file_access};
    use crate::modules::identity::domain::services::ResourceAccessEvaluator;
    use crate::modules::identity::domain::value_objects::ResourceGrantScope;
    use crate::modules::search::SearchVisibilityScope;
    use crate::modules::shared_kernel::domain::{EnvironmentId, NodeId, ProjectId};

    #[test]
    fn identity_access_is_translated_once_into_the_search_owned_contract() {
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let node_id = NodeId::new();
        assert!(
            search_visibility(&ResourceAccessEvaluator::organization_wide()).is_organization_wide()
        );
        let visibility = search_visibility(&ResourceAccessEvaluator::restricted([
            ResourceGrantScope::Project { project_id },
            ResourceGrantScope::Environment {
                project_id,
                environment_id,
            },
            ResourceGrantScope::Node { node_id },
        ]));

        assert_eq!(
            visibility.granted_scopes().collect::<Vec<_>>(),
            [
                SearchVisibilityScope::Project { project_id },
                SearchVisibilityScope::Environment {
                    project_id,
                    environment_id,
                },
                SearchVisibilityScope::Node { node_id },
            ]
        );
    }

    #[test]
    fn identity_access_is_narrowed_into_the_files_owned_projection() {
        let project_id = ProjectId::new();
        let environment_project_id = ProjectId::new();
        let access = user_file_access(&ResourceAccessEvaluator::restricted([
            ResourceGrantScope::Project { project_id },
            ResourceGrantScope::Environment {
                project_id: environment_project_id,
                environment_id: EnvironmentId::new(),
            },
            ResourceGrantScope::Node {
                node_id: NodeId::new(),
            },
        ]));

        assert!(access.project_is_visible(project_id));
        assert!(!access.project_is_visible(environment_project_id));
        assert!(!access.project_is_visible(ProjectId::new()));
        assert!(!access.organization_quota_is_visible());

        let organization_wide = user_file_access(&ResourceAccessEvaluator::organization_wide());
        assert!(organization_wide.project_is_visible(ProjectId::new()));
        assert!(organization_wide.organization_quota_is_visible());
    }
}
