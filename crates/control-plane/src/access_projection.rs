//! Root anti-corruption mappings from Identity decisions into consumer-owned access values.
//!
//! Identity is the authorization authority, while each bounded context owns the vocabulary it
//! needs to enforce resource visibility. Outer adapters use these projections at context entry;
//! consumer application and domain layers never depend on Identity grant types.

use crate::modules::agents::{AgentAccess, AgentAccessScope};
use crate::modules::applications::{ApplicationAccess, ApplicationAccessScope};
use crate::modules::artifacts::{ArtifactAccess, ArtifactAccessScope};
use crate::modules::assets::AssetAccess;
use crate::modules::connectors::{ConnectorAccess, ConnectorAccessScope};
use crate::modules::developer_workflows::{DeveloperWorkflowAccess, DeveloperWorkflowAccessScope};
use crate::modules::durable_cells::{DurableCellAccess, DurableCellAccessScope};
use crate::modules::executions::{ExecutionAccess, ExecutionAccessScope};
use crate::modules::files::UserFileAccess;
use crate::modules::fleet::{FleetAccess, FleetAccessScope};
use crate::modules::forms::{FormAccess, FormAccessScope};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::identity::domain::value_objects::ResourceGrantScope;
use crate::modules::notifications::{NotificationAccess, NotificationAccessScope};
use crate::modules::operations::{OperationAccess, OperationAccessScope};
use crate::modules::projects::{ProjectAccess, ProjectAccessScope};
use crate::modules::search::{SearchVisibility, SearchVisibilityScope};
use crate::modules::secrets::{SecretAccess, SecretAccessScope};
use crate::modules::workflow::{WorkflowAccess, WorkflowAccessScope};
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

pub(crate) fn secret_access(resource_access: &ResourceAccessEvaluator) -> SecretAccess {
    if resource_access.is_organization_wide() {
        return SecretAccess::organization_wide();
    }
    SecretAccess::restricted(
        resource_access
            .granted_scopes()
            .filter_map(|scope| match scope {
                ResourceGrantScope::Project { project_id } => {
                    Some(SecretAccessScope::Project { project_id })
                }
                ResourceGrantScope::Environment {
                    project_id,
                    environment_id,
                } => Some(SecretAccessScope::Environment {
                    project_id,
                    environment_id,
                }),
                ResourceGrantScope::Node { .. } => None,
            }),
    )
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

pub(crate) fn form_access(resource_access: &ResourceAccessEvaluator) -> FormAccess {
    if resource_access.is_organization_wide() {
        return FormAccess::organization_wide();
    }
    FormAccess::restricted(
        resource_access
            .granted_scopes()
            .filter_map(|scope| match scope {
                ResourceGrantScope::Project { project_id } => {
                    Some(FormAccessScope::Project { project_id })
                }
                ResourceGrantScope::Environment { .. } | ResourceGrantScope::Node { .. } => None,
            }),
    )
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

pub(crate) fn operation_access(resource_access: &ResourceAccessEvaluator) -> OperationAccess {
    if resource_access.is_organization_wide() {
        return OperationAccess::organization_wide();
    }
    OperationAccess::restricted(
        resource_access
            .granted_scopes()
            .filter_map(|scope| match scope {
                ResourceGrantScope::Project { project_id } => {
                    Some(OperationAccessScope::Project { project_id })
                }
                ResourceGrantScope::Environment {
                    project_id,
                    environment_id,
                } => Some(OperationAccessScope::Environment {
                    project_id,
                    environment_id,
                }),
                ResourceGrantScope::Node { .. } => None,
            }),
    )
}

pub(crate) fn project_access(resource_access: &ResourceAccessEvaluator) -> ProjectAccess {
    if resource_access.is_organization_wide() {
        return ProjectAccess::organization_wide();
    }
    ProjectAccess::restricted(
        resource_access
            .granted_scopes()
            .filter_map(|scope| match scope {
                ResourceGrantScope::Project { project_id } => {
                    Some(ProjectAccessScope::Project { project_id })
                }
                ResourceGrantScope::Environment {
                    project_id,
                    environment_id,
                } => Some(ProjectAccessScope::Environment {
                    project_id,
                    environment_id,
                }),
                ResourceGrantScope::Node { .. } => None,
            }),
    )
}

pub(crate) fn fleet_access(resource_access: &ResourceAccessEvaluator) -> FleetAccess {
    if resource_access.is_organization_wide() {
        return FleetAccess::organization_wide();
    }
    FleetAccess::restricted(
        resource_access
            .granted_scopes()
            .filter_map(|scope| match scope {
                ResourceGrantScope::Node { node_id } => Some(FleetAccessScope::Node { node_id }),
                ResourceGrantScope::Project { .. } | ResourceGrantScope::Environment { .. } => None,
            }),
    )
}

pub(crate) fn application_access(resource_access: &ResourceAccessEvaluator) -> ApplicationAccess {
    if resource_access.is_organization_wide() {
        return ApplicationAccess::organization_wide();
    }
    ApplicationAccess::restricted(resource_access.granted_scopes().filter_map(
        |scope| match scope {
            ResourceGrantScope::Project { project_id } => {
                Some(ApplicationAccessScope::Project { project_id })
            }
            ResourceGrantScope::Environment {
                project_id,
                environment_id,
            } => Some(ApplicationAccessScope::Environment {
                project_id,
                environment_id,
            }),
            ResourceGrantScope::Node { .. } => None,
        },
    ))
}

pub(crate) fn notification_access(resource_access: &ResourceAccessEvaluator) -> NotificationAccess {
    if resource_access.is_organization_wide() {
        return NotificationAccess::organization_wide();
    }
    NotificationAccess::restricted(resource_access.granted_scopes().map(|scope| match scope {
        ResourceGrantScope::Project { project_id } => {
            NotificationAccessScope::Project { project_id }
        }
        ResourceGrantScope::Environment {
            project_id,
            environment_id,
        } => NotificationAccessScope::Environment {
            project_id,
            environment_id,
        },
        ResourceGrantScope::Node { node_id } => NotificationAccessScope::Node { node_id },
    }))
}

pub(crate) fn durable_cell_access(resource_access: &ResourceAccessEvaluator) -> DurableCellAccess {
    if resource_access.is_organization_wide() {
        return DurableCellAccess::organization_wide();
    }
    DurableCellAccess::restricted(resource_access.granted_scopes().filter_map(
        |scope| match scope {
            ResourceGrantScope::Project { project_id } => {
                Some(DurableCellAccessScope::Project { project_id })
            }
            ResourceGrantScope::Environment {
                project_id,
                environment_id,
            } => Some(DurableCellAccessScope::Environment {
                project_id,
                environment_id,
            }),
            ResourceGrantScope::Node { .. } => None,
        },
    ))
}

pub(crate) fn connector_access(resource_access: &ResourceAccessEvaluator) -> ConnectorAccess {
    if resource_access.is_organization_wide() {
        return ConnectorAccess::organization_wide();
    }
    ConnectorAccess::restricted(
        resource_access
            .granted_scopes()
            .filter_map(|scope| match scope {
                ResourceGrantScope::Project { project_id } => {
                    Some(ConnectorAccessScope::Project { project_id })
                }
                ResourceGrantScope::Environment {
                    project_id,
                    environment_id,
                } => Some(ConnectorAccessScope::Environment {
                    project_id,
                    environment_id,
                }),
                ResourceGrantScope::Node { .. } => None,
            }),
    )
}

pub(crate) fn execution_access(resource_access: &ResourceAccessEvaluator) -> ExecutionAccess {
    if resource_access.is_organization_wide() {
        return ExecutionAccess::organization_wide();
    }
    ExecutionAccess::restricted(
        resource_access
            .granted_scopes()
            .filter_map(|scope| match scope {
                ResourceGrantScope::Project { project_id } => {
                    Some(ExecutionAccessScope::Project { project_id })
                }
                ResourceGrantScope::Environment {
                    project_id,
                    environment_id,
                } => Some(ExecutionAccessScope::Environment {
                    project_id,
                    environment_id,
                }),
                ResourceGrantScope::Node { .. } => None,
            }),
    )
}

pub(crate) fn agent_access(resource_access: &ResourceAccessEvaluator) -> AgentAccess {
    if resource_access.is_organization_wide() {
        return AgentAccess::organization_wide();
    }
    AgentAccess::restricted(
        resource_access
            .granted_scopes()
            .filter_map(|scope| match scope {
                ResourceGrantScope::Project { project_id } => {
                    Some(AgentAccessScope::Project { project_id })
                }
                ResourceGrantScope::Environment {
                    project_id,
                    environment_id,
                } => Some(AgentAccessScope::Environment {
                    project_id,
                    environment_id,
                }),
                ResourceGrantScope::Node { .. } => None,
            }),
    )
}

pub(crate) fn workflow_access(resource_access: &ResourceAccessEvaluator) -> WorkflowAccess {
    if resource_access.is_organization_wide() {
        return WorkflowAccess::organization_wide();
    }
    WorkflowAccess::restricted(
        resource_access
            .granted_scopes()
            .filter_map(|scope| match scope {
                ResourceGrantScope::Project { project_id } => {
                    Some(WorkflowAccessScope::Project { project_id })
                }
                ResourceGrantScope::Environment { .. } | ResourceGrantScope::Node { .. } => None,
            }),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        agent_access, application_access, artifact_access, asset_access, connector_access,
        developer_workflow_access, durable_cell_access, execution_access, fleet_access,
        form_access, notification_access, operation_access, project_access, search_visibility,
        secret_access, user_file_access, workflow_access, workload_access,
    };
    use crate::modules::identity::domain::services::ResourceAccessEvaluator;
    use crate::modules::identity::domain::value_objects::ResourceGrantScope;
    use crate::modules::search::SearchVisibilityScope;
    use crate::modules::shared_kernel::domain::{EnvironmentId, NodeId, ProjectId};

    #[test]
    fn identity_access_is_narrowed_into_the_assets_owned_projection() {
        assert!(
            asset_access(&ResourceAccessEvaluator::organization_wide())
                .organization_catalog_is_visible()
        );
        assert!(
            !asset_access(&ResourceAccessEvaluator::restricted([
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
            .organization_catalog_is_visible()
        );
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
    fn identity_access_is_narrowed_into_the_secrets_owned_projection() {
        let project_id = ProjectId::new();
        let environment_project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let access = secret_access(&ResourceAccessEvaluator::restricted([
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
        assert_eq!(access.granted_scopes().count(), 2);

        assert!(
            secret_access(&ResourceAccessEvaluator::organization_wide())
                .environment_is_visible(ProjectId::new(), EnvironmentId::new())
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
    fn identity_access_is_narrowed_into_the_forms_owned_projection() {
        let project_id = ProjectId::new();
        let environment_project_id = ProjectId::new();
        let access = form_access(&ResourceAccessEvaluator::restricted([
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

        let organization_wide = form_access(&ResourceAccessEvaluator::organization_wide());
        assert!(organization_wide.project_is_visible(ProjectId::new()));
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

    #[test]
    fn identity_access_is_narrowed_into_the_operations_owned_projection() {
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let access = operation_access(&ResourceAccessEvaluator::restricted([
            ResourceGrantScope::Project { project_id },
            ResourceGrantScope::Environment {
                project_id,
                environment_id,
            },
            ResourceGrantScope::Node {
                node_id: NodeId::new(),
            },
        ]));

        assert!(!access.is_organization_wide());
        assert_eq!(access.granted_scopes().count(), 2);
        assert!(
            operation_access(&ResourceAccessEvaluator::organization_wide()).is_organization_wide()
        );
    }

    #[test]
    fn identity_access_is_narrowed_into_the_executions_owned_projection() {
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let access = execution_access(&ResourceAccessEvaluator::restricted([
            ResourceGrantScope::Project { project_id },
            ResourceGrantScope::Environment {
                project_id,
                environment_id,
            },
            ResourceGrantScope::Node {
                node_id: NodeId::new(),
            },
        ]));

        assert!(access.environment_is_visible(project_id, environment_id));
        assert!(access.environment_is_visible(project_id, EnvironmentId::new()));
        assert!(!access.environment_is_visible(ProjectId::new(), environment_id));
        assert!(
            execution_access(&ResourceAccessEvaluator::organization_wide())
                .environment_is_visible(ProjectId::new(), EnvironmentId::new())
        );
    }

    #[test]
    fn identity_access_is_narrowed_into_the_agents_owned_projection() {
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let access = agent_access(&ResourceAccessEvaluator::restricted([
            ResourceGrantScope::Project { project_id },
            ResourceGrantScope::Environment {
                project_id,
                environment_id,
            },
            ResourceGrantScope::Node {
                node_id: NodeId::new(),
            },
        ]));

        assert!(access.environment_is_visible(project_id, environment_id));
        assert!(access.environment_is_visible(project_id, EnvironmentId::new()));
        assert!(!access.environment_is_visible(ProjectId::new(), environment_id));
        assert!(
            agent_access(&ResourceAccessEvaluator::organization_wide())
                .environment_is_visible(ProjectId::new(), EnvironmentId::new())
        );
    }

    #[test]
    fn identity_access_is_narrowed_into_the_workflows_owned_projection() {
        let project_id = ProjectId::new();
        let access = workflow_access(&ResourceAccessEvaluator::restricted([
            ResourceGrantScope::Project { project_id },
            ResourceGrantScope::Environment {
                project_id,
                environment_id: EnvironmentId::new(),
            },
            ResourceGrantScope::Node {
                node_id: NodeId::new(),
            },
        ]));

        assert!(access.project_is_visible(project_id));
        assert!(!access.project_is_visible(ProjectId::new()));
        assert!(!access.is_organization_wide());
        assert_eq!(access.granted_scopes().count(), 1);
        assert!(
            workflow_access(&ResourceAccessEvaluator::organization_wide())
                .project_is_visible(ProjectId::new())
        );
    }

    #[test]
    fn identity_access_is_narrowed_into_the_projects_owned_projection() {
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let access = project_access(&ResourceAccessEvaluator::restricted([
            ResourceGrantScope::Project { project_id },
            ResourceGrantScope::Environment {
                project_id,
                environment_id,
            },
            ResourceGrantScope::Node {
                node_id: NodeId::new(),
            },
        ]));

        assert!(access.project_is_visible_in_collection(project_id));
        assert!(access.project_is_authorized(project_id));
        assert!(access.environment_is_visible(project_id, environment_id));
        assert!(access.environment_is_visible(project_id, EnvironmentId::new()));

        let environment_only = project_access(&ResourceAccessEvaluator::restricted([
            ResourceGrantScope::Environment {
                project_id,
                environment_id,
            },
        ]));
        assert!(environment_only.project_is_visible_in_collection(project_id));
        assert!(!environment_only.project_is_authorized(project_id));
        assert!(environment_only.environment_is_visible(project_id, environment_id));
        assert!(!environment_only.environment_is_visible(project_id, EnvironmentId::new()));
    }

    #[test]
    fn identity_access_is_narrowed_into_the_fleet_owned_projection() {
        let node_id = NodeId::new();
        let access = fleet_access(&ResourceAccessEvaluator::restricted([
            ResourceGrantScope::Project {
                project_id: ProjectId::new(),
            },
            ResourceGrantScope::Environment {
                project_id: ProjectId::new(),
                environment_id: EnvironmentId::new(),
            },
            ResourceGrantScope::Node { node_id },
        ]));

        assert!(!access.is_organization_wide());
        assert!(access.node_is_visible(node_id));
        assert!(!access.node_is_visible(NodeId::new()));
        assert_eq!(access.granted_scopes().count(), 1);
        assert!(fleet_access(&ResourceAccessEvaluator::organization_wide()).is_organization_wide());
        assert!(
            fleet_access(&ResourceAccessEvaluator::organization_wide())
                .node_is_visible(NodeId::new())
        );
    }

    #[test]
    fn identity_access_is_narrowed_into_the_applications_owned_projection() {
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let access = application_access(&ResourceAccessEvaluator::restricted([
            ResourceGrantScope::Project { project_id },
            ResourceGrantScope::Environment {
                project_id,
                environment_id,
            },
            ResourceGrantScope::Node {
                node_id: NodeId::new(),
            },
        ]));

        assert!(access.project_is_authorized(project_id));
        assert!(access.environment_is_visible(project_id, environment_id));
        assert!(access.environment_is_visible(project_id, EnvironmentId::new()));
        assert_eq!(access.granted_scopes().count(), 2);

        let environment_only = application_access(&ResourceAccessEvaluator::restricted([
            ResourceGrantScope::Environment {
                project_id,
                environment_id,
            },
        ]));
        assert!(!environment_only.project_is_authorized(project_id));
        assert!(environment_only.environment_is_visible(project_id, environment_id));
        assert!(!environment_only.environment_is_visible(project_id, EnvironmentId::new()));
        assert!(
            application_access(&ResourceAccessEvaluator::organization_wide())
                .project_is_authorized(ProjectId::new())
        );
    }

    #[test]
    fn identity_access_is_narrowed_into_the_notifications_owned_projection() {
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let node_id = NodeId::new();
        let access = notification_access(&ResourceAccessEvaluator::restricted([
            ResourceGrantScope::Project { project_id },
            ResourceGrantScope::Environment {
                project_id,
                environment_id,
            },
            ResourceGrantScope::Node { node_id },
        ]));

        assert!(access.project_is_authorized(project_id));
        assert!(access.environment_is_visible(project_id, environment_id));
        assert!(access.node_is_visible(node_id));
        assert_eq!(access.granted_scopes().count(), 3);
        assert!(
            notification_access(&ResourceAccessEvaluator::organization_wide())
                .is_organization_wide()
        );
    }

    #[test]
    fn identity_access_is_narrowed_into_the_durable_cells_owned_projection() {
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let access = durable_cell_access(&ResourceAccessEvaluator::restricted([
            ResourceGrantScope::Project { project_id },
            ResourceGrantScope::Environment {
                project_id,
                environment_id,
            },
            ResourceGrantScope::Node {
                node_id: NodeId::new(),
            },
        ]));

        assert!(access.environment_is_visible(project_id, environment_id));
        assert!(access.environment_is_visible(project_id, EnvironmentId::new()));
        assert_eq!(access.granted_scopes().count(), 2);

        let environment_only = durable_cell_access(&ResourceAccessEvaluator::restricted([
            ResourceGrantScope::Environment {
                project_id,
                environment_id,
            },
        ]));
        assert!(environment_only.environment_is_visible(project_id, environment_id));
        assert!(!environment_only.environment_is_visible(project_id, EnvironmentId::new()));
        assert!(
            durable_cell_access(&ResourceAccessEvaluator::organization_wide())
                .is_organization_wide()
        );
    }

    #[test]
    fn identity_access_is_narrowed_into_the_connectors_owned_projection() {
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let access = connector_access(&ResourceAccessEvaluator::restricted([
            ResourceGrantScope::Project { project_id },
            ResourceGrantScope::Environment {
                project_id,
                environment_id,
            },
            ResourceGrantScope::Node {
                node_id: NodeId::new(),
            },
        ]));

        assert!(access.environment_is_visible(project_id, environment_id));
        assert!(access.environment_is_visible(project_id, EnvironmentId::new()));
        assert_eq!(access.granted_scopes().count(), 2);

        let environment_only = connector_access(&ResourceAccessEvaluator::restricted([
            ResourceGrantScope::Environment {
                project_id,
                environment_id,
            },
        ]));
        assert!(environment_only.environment_is_visible(project_id, environment_id));
        assert!(!environment_only.environment_is_visible(project_id, EnvironmentId::new()));
        assert!(
            connector_access(&ResourceAccessEvaluator::organization_wide()).is_organization_wide()
        );
    }
}
