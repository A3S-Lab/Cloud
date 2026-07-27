use crate::modules::identity::domain::value_objects::ApiTokenScope;
use a3s_boot::AuthPrincipal;
use serde_json::{json, Value};

pub const ENVIRONMENTS_CREATE: &str = "a3s_cloud_environments_create";
pub const ENVIRONMENTS_LIST: &str = "a3s_cloud_environments_list";
pub const PROJECTS_CREATE: &str = "a3s_cloud_projects_create";
pub const PROJECTS_LIST: &str = "a3s_cloud_projects_list";
pub const SEARCH: &str = "a3s_cloud_search";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagementTool {
    EnvironmentsCreate,
    EnvironmentsList,
    ProjectsCreate,
    ProjectsList,
    Search,
}

impl ManagementTool {
    const ALL: [Self; 5] = [
        Self::EnvironmentsCreate,
        Self::EnvironmentsList,
        Self::ProjectsCreate,
        Self::ProjectsList,
        Self::Search,
    ];

    pub fn visible_to(self, principal: &AuthPrincipal) -> bool {
        self.required_scope()
            .is_none_or(|scope| principal.has_scope(scope))
    }

    pub fn resolve(name: &str, principal: &AuthPrincipal) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|tool| tool.name() == name && tool.visible_to(principal))
    }

    pub fn visible_catalog(principal: &AuthPrincipal) -> Vec<Value> {
        Self::ALL
            .into_iter()
            .filter(|tool| tool.visible_to(principal))
            .map(Self::definition)
            .collect()
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::EnvironmentsCreate => ENVIRONMENTS_CREATE,
            Self::EnvironmentsList => ENVIRONMENTS_LIST,
            Self::ProjectsCreate => PROJECTS_CREATE,
            Self::ProjectsList => PROJECTS_LIST,
            Self::Search => SEARCH,
        }
    }

    const fn required_scope(self) -> Option<&'static str> {
        match self {
            Self::EnvironmentsCreate => Some(ApiTokenScope::ENVIRONMENT_WRITE),
            Self::ProjectsCreate => Some(ApiTokenScope::PROJECT_WRITE),
            Self::EnvironmentsList | Self::ProjectsList | Self::Search => None,
        }
    }

    fn definition(self) -> Value {
        let (title, description, input_schema, read_only) = match self {
            Self::EnvironmentsCreate => (
                "Create environment",
                "Create an environment in one tenant-authorized project with explicit idempotency.",
                create_environment_schema(),
                false,
            ),
            Self::EnvironmentsList => (
                "List environments",
                "List environments in one tenant-authorized project.",
                project_id_schema(),
                true,
            ),
            Self::ProjectsCreate => (
                "Create project",
                "Create a project in the authenticated organization with explicit idempotency.",
                create_project_schema(),
                false,
            ),
            Self::ProjectsList => (
                "List projects",
                "List projects in the authenticated organization.",
                empty_schema(),
                true,
            ),
            Self::Search => (
                "Search Cloud resources",
                "Search bounded tenant-authorized resource projections in the authenticated organization.",
                search_schema(),
                true,
            ),
        };
        json!({
            "name": self.name(),
            "title": title,
            "description": description,
            "inputSchema": input_schema,
            "annotations": {
                "readOnlyHint": read_only,
                "destructiveHint": false,
                "idempotentHint": true,
                "openWorldHint": false
            }
        })
    }
}

fn empty_schema() -> Value {
    json!({
        "type": "object",
        "properties": {},
        "additionalProperties": false
    })
}

fn project_id_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "projectId": {"type": "string", "format": "uuid"}
        },
        "required": ["projectId"],
        "additionalProperties": false
    })
}

fn create_project_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "name": {"type": "string", "minLength": 1, "maxLength": 100},
            "idempotencyKey": {"type": "string", "minLength": 1, "maxLength": 255}
        },
        "required": ["name", "idempotencyKey"],
        "additionalProperties": false
    })
}

fn create_environment_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "projectId": {"type": "string", "format": "uuid"},
            "name": {"type": "string", "minLength": 1, "maxLength": 100},
            "idempotencyKey": {"type": "string", "minLength": 1, "maxLength": 255}
        },
        "required": ["projectId", "name", "idempotencyKey"],
        "additionalProperties": false
    })
}

fn search_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "query": {"type": "string", "minLength": 1, "maxLength": 128},
            "limit": {"type": "integer", "minimum": 1, "maximum": 50, "default": 20}
        },
        "required": ["query"],
        "additionalProperties": false
    })
}
