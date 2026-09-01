//! Root anti-corruption mappings from Identity decisions into consumer-owned access values.
//!
//! Identity is the authorization authority, while each bounded context owns the vocabulary it
//! needs to enforce resource visibility. Outer adapters use these projections at context entry;
//! consumer application and domain layers never depend on Identity grant types.

use crate::modules::artifacts::{ArtifactAccess, ArtifactAccessScope};
use crate::modules::assets::AssetAccess;
use crate::modules::developer_workflows::{DeveloperWorkflowAccess, DeveloperWorkflowAccessScope};
use crate::modules::files::UserFileAccess;
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::identity::domain::value_objects::ResourceGrantScope;
use crate::modules::search::{SearchVisibility, SearchVisibilityScope};
use crate::modules::workloads::{WorkloadAccess, WorkloadAccessScope};

pub(crate) fn asset_access(resource_access: &ResourceAccessEvaluator) -> AssetAccess {
    if resource_access.is_organization_wide() {
        AssetAccess::organization_wide()
    } else {
        AssetAccess::restricted()
    }
}

pub(crate) fn artifact_access(resource_access: &ResourceAccessEvaluator) -> ArtifactAccess {
    if resource_access.is_organization_wide() {
        return ArtifactAccess::organization_wide();
    }
    ArtifactAccess::restricted(
        resource_access
            .granted_scopes()
            .filter_map(|scope| match scope {
                ResourceGrantScope::Project { project_id } => {
                    Some(ArtifactAccessScope::Project { project_id })
                }
                ResourceGrantScope::Environment {
                    project_id,
                    environment_id,
                } => Some(ArtifactAccessScope::Environment {
                    project_id,
                    environment_id,
                }),
                ResourceGrantScope::Node { .. } => None,
            }),
    )
}

pub(crate) fn developer_workflow_access(
    resource_access: &ResourceAccessEvaluator,
) -> DeveloperWorkflowAccess {
    if resource_access.is_organization_wide() {
        return DeveloperWorkflowAccess::organization_wide();
    }
    DeveloperWorkflowAccess::restricted(resource_access.granted_scopes().filter_map(|scope| {
        match scope {
            ResourceGrantScope::Project { project_id } => {
                Some(DeveloperWorkflowAccessScope::Project { project_id })
            }
            ResourceGrantScope::Environment {
                project_id,
                environment_id,
            } => Some(DeveloperWorkflowAccessScope::Environment {
                project_id,
                environment_id,
            }),
            ResourceGrantScope::Node { .. } => None,
        }
    }))
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

pub(crate) fn workload_access(resource_access: &ResourceAccessEvaluator) -> WorkloadAccess {
    if resource_access.is_organization_wide() {
        return WorkloadAccess::organization_wide();
    }
    WorkloadAccess::restricted(
        resource_access
            .granted_scopes()
            .filter_map(|scope| match scope {
                ResourceGrantScope::Project { project_id } => {
                    Some(WorkloadAccessScope::Project { project_id })
                }
                ResourceGrantScope::Environment {
                    project_id,
                    environment_id,
                } => Some(WorkloadAccessScope::Environment {
                    project_id,
                    environment_id,
                }),
                ResourceGrantScope::Node { .. } => None,
            }),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        artifact_access, asset_access, developer_workflow_access, search_visibility,
        user_file_access, workload_access,
    };
    use crate::modules::identity::domain::services::ResourceAccessEvaluator;
    use crate::modules::identity::domain::value_objects::ResourceGrantScope;
    use crate::modules::search::SearchVisibilityScope;
    use crate::modules::shared_kernel::domain::{EnvironmentId, NodeId, ProjectId};

    #[test]
    fn identity_access_is_narrowed_into_the_assets_owned_projection() {
        assert!(asset_access(&ResourceAccessEvaluator::organization_wide())
            .organization_catalog_is_visible());
        assert!(!asset_access(&ResourceAccessEvaluator::restricted([
            ResourceGrantScope::Project {
                project_id: ProjectId::new(),
            },
            ResourceGrantScope::Environment {
                project_id: ProjectId::new(),
                environment_id: EnvironmentId::new(),
            },
            ResourceGrantScope::Node {
                node_id: NodeId::new(),
            },
        ]))
        .organization_catalog_is_visible());
    }

    #[test]
    fn identity_access_is_narrowed_into_the_artifacts_owned_projection() {
        let project_id = ProjectId::new();
        let environment_project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let access = artifact_access(&ResourceAccessEvaluator::restricted([
            ResourceGrantScope::Project { project_id },
            ResourceGrantScope::Environment {
                project_id: environment_project_id,
                environment_id,
            },
            ResourceGrantScope::Node {
                node_id: NodeId::new(),
            },
        ]));

        assert!(access.environment_is_visible(project_id, EnvironmentId::new()));
        assert!(access.environment_is_visible(environment_project_id, environment_id));
        assert!(!access.environment_is_visible(environment_project_id, EnvironmentId::new()));
        assert!(!access.organization_build_is_visible());
        assert_eq!(access.granted_scopes().count(), 2);

        let organization_wide = artifact_access(&ResourceAccessEvaluator::organization_wide());
        assert!(organization_wide.environment_is_visible(ProjectId::new(), EnvironmentId::new()));
        assert!(organization_wide.organization_build_is_visible());
    }

    #[test]
    fn identity_access_is_narrowed_into_the_developer_workflows_owned_projection() {
        let project_id = ProjectId::new();
        let environment_project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let access = developer_workflow_access(&ResourceAccessEvaluator::restricted([
            ResourceGrantScope::Project { project_id },
            ResourceGrantScope::Environment {
                project_id: environment_project_id,
                environment_id,
            },
            ResourceGrantScope::Node {
                node_id: NodeId::new(),
            },
        ]));

        assert!(access.environment_is_visible(project_id, EnvironmentId::new()));
        assert!(access.environment_is_visible(environment_project_id, environment_id));
        assert!(!access.environment_is_visible(environment_project_id, EnvironmentId::new()));
        assert!(!access.environment_is_visible(ProjectId::new(), EnvironmentId::new()));

        let organization_wide =
            developer_workflow_access(&ResourceAccessEvaluator::organization_wide());
        assert!(organization_wide.environment_is_visible(ProjectId::new(), EnvironmentId::new()));
    }

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

    #[test]
    fn identity_access_is_narrowed_into_the_workloads_owned_projection() {
        let project_id = ProjectId::new();
        let environment_project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let access = workload_access(&ResourceAccessEvaluator::restricted([
            ResourceGrantScope::Project { project_id },
            ResourceGrantScope::Environment {
                project_id: environment_project_id,
                environment_id,
            },
            ResourceGrantScope::Node {
                node_id: NodeId::new(),
            },
        ]));

        assert!(access.environment_is_visible(project_id, EnvironmentId::new()));
        assert!(access.environment_is_visible(environment_project_id, environment_id));
        assert!(!access.environment_is_visible(environment_project_id, EnvironmentId::new()));
        assert!(!access.environment_is_visible(ProjectId::new(), EnvironmentId::new()));

        let organization_wide = workload_access(&ResourceAccessEvaluator::organization_wide());
        assert!(organization_wide.environment_is_visible(ProjectId::new(), EnvironmentId::new()));
    }
}
