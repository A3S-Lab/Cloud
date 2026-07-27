use crate::modules::identity::domain::value_objects::ApiTokenScope;
use a3s_boot::AuthPrincipal;
use serde_json::{json, Value};

pub const BUILD_RUNS_GET: &str = "a3s_cloud_build_runs_get";
pub const BUILD_RUNS_LIST: &str = "a3s_cloud_build_runs_list";
pub const DEPLOYMENTS_GET: &str = "a3s_cloud_deployments_get";
pub const ENVIRONMENTS_CREATE: &str = "a3s_cloud_environments_create";
pub const ENVIRONMENTS_LIST: &str = "a3s_cloud_environments_list";
pub const NODES_GET: &str = "a3s_cloud_nodes_get";
pub const NODES_LIST: &str = "a3s_cloud_nodes_list";
pub const OPERATIONS_LIST: &str = "a3s_cloud_operations_list";
pub const PROJECTS_CREATE: &str = "a3s_cloud_projects_create";
pub const PROJECTS_LIST: &str = "a3s_cloud_projects_list";
pub const ROUTES_GET: &str = "a3s_cloud_routes_get";
pub const ROUTES_LIST: &str = "a3s_cloud_routes_list";
pub const SEARCH: &str = "a3s_cloud_search";
pub const WORKLOADS_GET: &str = "a3s_cloud_workloads_get";
pub const WORKLOADS_LIST: &str = "a3s_cloud_workloads_list";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagementTool {
    EnvironmentsCreate,
    EnvironmentsList,
    ProjectsCreate,
    ProjectsList,
    Search,
    NodesList,
    NodesGet,
    OperationsList,
    WorkloadsList,
    WorkloadsGet,
    DeploymentsGet,
    RoutesList,
    RoutesGet,
    BuildRunsList,
    BuildRunsGet,
}

impl ManagementTool {
    const ALL: [Self; 15] = [
        Self::EnvironmentsCreate,
        Self::EnvironmentsList,
        Self::ProjectsCreate,
        Self::ProjectsList,
        Self::Search,
        Self::NodesList,
        Self::NodesGet,
        Self::OperationsList,
        Self::WorkloadsList,
        Self::WorkloadsGet,
        Self::DeploymentsGet,
        Self::RoutesList,
        Self::RoutesGet,
        Self::BuildRunsList,
        Self::BuildRunsGet,
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
            Self::NodesList => NODES_LIST,
            Self::NodesGet => NODES_GET,
            Self::OperationsList => OPERATIONS_LIST,
            Self::WorkloadsList => WORKLOADS_LIST,
            Self::WorkloadsGet => WORKLOADS_GET,
            Self::DeploymentsGet => DEPLOYMENTS_GET,
            Self::RoutesList => ROUTES_LIST,
            Self::RoutesGet => ROUTES_GET,
            Self::BuildRunsList => BUILD_RUNS_LIST,
            Self::BuildRunsGet => BUILD_RUNS_GET,
        }
    }

    const fn required_scope(self) -> Option<&'static str> {
        match self {
            Self::EnvironmentsCreate => Some(ApiTokenScope::ENVIRONMENT_WRITE),
            Self::ProjectsCreate => Some(ApiTokenScope::PROJECT_WRITE),
            Self::EnvironmentsList
            | Self::ProjectsList
            | Self::Search
            | Self::NodesList
            | Self::NodesGet
            | Self::OperationsList
            | Self::WorkloadsList
            | Self::WorkloadsGet
            | Self::DeploymentsGet
            | Self::RoutesList
            | Self::RoutesGet
            | Self::BuildRunsList
            | Self::BuildRunsGet => None,
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
            Self::NodesList => (
                "List nodes",
                "List node inventory and current availability in the authenticated organization.",
                empty_schema(),
                true,
            ),
            Self::NodesGet => (
                "Get node",
                "Get one tenant-authorized node and its current availability.",
                uuid_id_schema("nodeId"),
                true,
            ),
            Self::OperationsList => (
                "List operations",
                "List a bounded snapshot of recent operations in the authenticated organization.",
                bounded_limit_schema(),
                true,
            ),
            Self::WorkloadsList => (
                "List workloads",
                "List workloads in one tenant-authorized environment.",
                environment_scope_schema(),
                true,
            ),
            Self::WorkloadsGet => (
                "Get workload",
                "Get one tenant-authorized workload and its deployment state.",
                uuid_id_schema("workloadId"),
                true,
            ),
            Self::DeploymentsGet => (
                "Get deployment",
                "Get one tenant-authorized deployment and its observed operation state.",
                uuid_id_schema("deploymentId"),
                true,
            ),
            Self::RoutesList => (
                "List routes",
                "List routes in one tenant-authorized environment.",
                environment_scope_schema(),
                true,
            ),
            Self::RoutesGet => (
                "Get route",
                "Get one tenant-authorized route and its Gateway publication state.",
                uuid_id_schema("routeId"),
                true,
            ),
            Self::BuildRunsList => (
                "List build runs",
                "List a bounded set of BuildRuns in one tenant-authorized environment.",
                build_run_list_schema(),
                true,
            ),
            Self::BuildRunsGet => (
                "Get build run",
                "Get one tenant-authorized BuildRun and its publication summary.",
                uuid_id_schema("buildRunId"),
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

fn environment_scope_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "projectId": {"type": "string", "format": "uuid"},
            "environmentId": {"type": "string", "format": "uuid"}
        },
        "required": ["projectId", "environmentId"],
        "additionalProperties": false
    })
}

fn uuid_id_schema(property: &str) -> Value {
    let mut properties = serde_json::Map::new();
    properties.insert(property.into(), json!({"type": "string", "format": "uuid"}));
    json!({
        "type": "object",
        "properties": properties,
        "required": [property],
        "additionalProperties": false
    })
}

fn bounded_limit_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "limit": {"type": "integer", "minimum": 1, "maximum": 200, "default": 50}
        },
        "additionalProperties": false
    })
}

fn build_run_list_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "projectId": {"type": "string", "format": "uuid"},
            "environmentId": {"type": "string", "format": "uuid"},
            "limit": {"type": "integer", "minimum": 1, "maximum": 200, "default": 50}
        },
        "required": ["projectId", "environmentId"],
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
