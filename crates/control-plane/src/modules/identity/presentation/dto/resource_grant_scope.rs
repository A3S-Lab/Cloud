use crate::modules::identity::domain::value_objects::ResourceGrantScope;
use crate::modules::shared_kernel::domain::{ApplicationId, EnvironmentId, NodeId, ProjectId};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ResourceGrantScopeDto {
    Project {
        #[serde(rename = "projectId")]
        project_id: Uuid,
    },
    Environment {
        #[serde(rename = "projectId")]
        project_id: Uuid,
        #[serde(rename = "environmentId")]
        environment_id: Uuid,
    },
    Application {
        #[serde(rename = "projectId")]
        project_id: Uuid,
        #[serde(rename = "applicationId")]
        application_id: Uuid,
    },
    Node {
        #[serde(rename = "nodeId")]
        node_id: Uuid,
    },
}

impl TryFrom<ResourceGrantScopeDto> for ResourceGrantScope {
    type Error = String;

    fn try_from(scope: ResourceGrantScopeDto) -> Result<Self, Self::Error> {
        match scope {
            ResourceGrantScopeDto::Project { project_id } if !project_id.is_nil() => {
                Ok(Self::Project {
                    project_id: ProjectId::from_uuid(project_id),
                })
            }
            ResourceGrantScopeDto::Environment {
                project_id,
                environment_id,
            } if !project_id.is_nil() && !environment_id.is_nil() => Ok(Self::Environment {
                project_id: ProjectId::from_uuid(project_id),
                environment_id: EnvironmentId::from_uuid(environment_id),
            }),
            ResourceGrantScopeDto::Application {
                project_id,
                application_id,
            } if !project_id.is_nil() && !application_id.is_nil() => Ok(Self::Application {
                project_id: ProjectId::from_uuid(project_id),
                application_id: ApplicationId::from_uuid(application_id),
            }),
            ResourceGrantScopeDto::Node { node_id } if !node_id.is_nil() => Ok(Self::Node {
                node_id: NodeId::from_uuid(node_id),
            }),
            _ => Err("Resource Grant scope identifiers must not be nil".into()),
        }
    }
}

impl From<ResourceGrantScope> for ResourceGrantScopeDto {
    fn from(scope: ResourceGrantScope) -> Self {
        match scope {
            ResourceGrantScope::Project { project_id } => Self::Project {
                project_id: project_id.as_uuid(),
            },
            ResourceGrantScope::Environment {
                project_id,
                environment_id,
            } => Self::Environment {
                project_id: project_id.as_uuid(),
                environment_id: environment_id.as_uuid(),
            },
            ResourceGrantScope::Application {
                project_id,
                application_id,
            } => Self::Application {
                project_id: project_id.as_uuid(),
                application_id: application_id.as_uuid(),
            },
            ResourceGrantScope::Node { node_id } => Self::Node {
                node_id: node_id.as_uuid(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_scope_is_camel_case_and_closed() {
        let project_id = Uuid::now_v7();
        let value = serde_json::to_value(ResourceGrantScopeDto::Environment {
            project_id,
            environment_id: Uuid::now_v7(),
        })
        .expect("scope");
        assert_eq!(value["kind"], "environment");
        assert_eq!(value["projectId"], project_id.to_string());
        assert!(value.get("project_id").is_none());
        assert!(
            serde_json::from_value::<ResourceGrantScopeDto>(serde_json::json!({
                "kind": "project",
                "projectId": project_id,
                "nodeId": Uuid::now_v7(),
            }))
            .is_err()
        );
    }

    #[test]
    fn application_scope_round_trips() {
        let project_id = Uuid::now_v7();
        let application_id = Uuid::now_v7();
        let dto = ResourceGrantScopeDto::Application {
            project_id,
            application_id,
        };
        let domain = ResourceGrantScope::try_from(dto).expect("dto");
        assert_eq!(
            ResourceGrantScopeDto::from(domain),
            ResourceGrantScopeDto::Application {
                project_id,
                application_id,
            }
        );
    }
}
