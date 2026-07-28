use super::arguments::{DEFAULT_LOG_LIMIT, MAXIMUM_IDEMPOTENCY_KEY_LENGTH, MAXIMUM_LOG_LIMIT};
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use a3s_boot::AuthPrincipal;
use serde_json::{json, Value};

pub const BUILD_RUNS_GET: &str = "a3s_cloud_build_runs_get";
pub const BUILD_RUNS_LIST: &str = "a3s_cloud_build_runs_list";
pub const BUILD_RUNS_CANCEL: &str = "a3s_cloud_build_runs_cancel";
pub const BUILD_RUNS_RETRY: &str = "a3s_cloud_build_runs_retry";
pub const BUILD_RUN_LOGS_GET: &str = "a3s_cloud_build_run_logs_get";
pub const BUILD_EVIDENCE_GET: &str = "a3s_cloud_build_evidence_get";
pub const DEPLOYMENTS_CANCEL: &str = "a3s_cloud_deployments_cancel";
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
pub const WORKLOADS_ROLLBACK: &str = "a3s_cloud_workloads_rollback";
pub const WORKLOADS_STOP: &str = "a3s_cloud_workloads_stop";
pub const WORKLOAD_LOGS_GET: &str = "a3s_cloud_workload_logs_get";

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
    WorkloadLogsGet,
    WorkloadsStop,
    WorkloadsRollback,
    DeploymentsGet,
    DeploymentsCancel,
    RoutesList,
    RoutesGet,
    BuildRunsList,
    BuildRunsGet,
    BuildRunLogsGet,
    BuildEvidenceGet,
    BuildRunsCancel,
    BuildRunsRetry,
}

impl ManagementTool {
    const ALL: [Self; 23] = [
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
        Self::WorkloadLogsGet,
        Self::WorkloadsStop,
        Self::WorkloadsRollback,
        Self::DeploymentsGet,
        Self::DeploymentsCancel,
        Self::RoutesList,
        Self::RoutesGet,
        Self::BuildRunsList,
        Self::BuildRunsGet,
        Self::BuildRunLogsGet,
        Self::BuildEvidenceGet,
        Self::BuildRunsCancel,
        Self::BuildRunsRetry,
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
            Self::WorkloadLogsGet => WORKLOAD_LOGS_GET,
            Self::WorkloadsStop => WORKLOADS_STOP,
            Self::WorkloadsRollback => WORKLOADS_ROLLBACK,
            Self::DeploymentsGet => DEPLOYMENTS_GET,
            Self::DeploymentsCancel => DEPLOYMENTS_CANCEL,
            Self::RoutesList => ROUTES_LIST,
            Self::RoutesGet => ROUTES_GET,
            Self::BuildRunsList => BUILD_RUNS_LIST,
            Self::BuildRunsGet => BUILD_RUNS_GET,
            Self::BuildRunLogsGet => BUILD_RUN_LOGS_GET,
            Self::BuildEvidenceGet => BUILD_EVIDENCE_GET,
            Self::BuildRunsCancel => BUILD_RUNS_CANCEL,
            Self::BuildRunsRetry => BUILD_RUNS_RETRY,
        }
    }

    const fn required_scope(self) -> Option<&'static str> {
        match self {
            Self::EnvironmentsCreate => Some(ApiTokenScope::ENVIRONMENT_WRITE),
            Self::ProjectsCreate => Some(ApiTokenScope::PROJECT_WRITE),
            Self::WorkloadsStop | Self::WorkloadsRollback | Self::DeploymentsCancel => {
                Some(ApiTokenScope::WORKLOAD_WRITE)
            }
            Self::BuildRunsCancel | Self::BuildRunsRetry => Some(ApiTokenScope::BUILD_WRITE),
            Self::EnvironmentsList
            | Self::ProjectsList
            | Self::Search
            | Self::NodesList
            | Self::NodesGet
            | Self::OperationsList
            | Self::WorkloadsList
            | Self::WorkloadsGet
            | Self::WorkloadLogsGet
            | Self::DeploymentsGet
            | Self::RoutesList
            | Self::RoutesGet
            | Self::BuildRunsList
            | Self::BuildRunsGet
            | Self::BuildRunLogsGet
            | Self::BuildEvidenceGet => None,
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
            Self::WorkloadLogsGet => (
                "Get workload logs",
                "Get one bounded page of retained logs for a tenant-authorized Workload revision.",
                workload_logs_schema(),
                true,
            ),
            Self::WorkloadsStop => (
                "Stop workload",
                "Stop one tenant-authorized Workload with explicit idempotency.",
                idempotent_uuid_id_schema("workloadId"),
                false,
            ),
            Self::WorkloadsRollback => (
                "Roll back workload",
                "Roll back one tenant-authorized Workload to an existing revision with explicit idempotency.",
                rollback_workload_schema(),
                false,
            ),
            Self::DeploymentsGet => (
                "Get deployment",
                "Get one tenant-authorized deployment and its observed operation state.",
                uuid_id_schema("deploymentId"),
                true,
            ),
            Self::DeploymentsCancel => (
                "Cancel deployment",
                "Cancel one tenant-authorized Deployment with explicit idempotency.",
                idempotent_uuid_id_schema("deploymentId"),
                false,
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
            Self::BuildRunLogsGet => (
                "Get build run logs",
                "Get one bounded page of retained logs for a tenant-authorized BuildRun.",
                build_run_logs_schema(),
                true,
            ),
            Self::BuildEvidenceGet => (
                "Get build evidence",
                "Get the signed evidence projection for a tenant-authorized BuildRun.",
                uuid_id_schema("buildRunId"),
                true,
            ),
            Self::BuildRunsCancel => (
                "Cancel build run",
                "Cancel one tenant-authorized BuildRun with explicit idempotency.",
                idempotent_uuid_id_schema("buildRunId"),
                false,
            ),
            Self::BuildRunsRetry => (
                "Retry build run",
                "Retry one tenant-authorized BuildRun with explicit idempotency.",
                idempotent_uuid_id_schema("buildRunId"),
                false,
            ),
        };
        let destructive = matches!(
            self,
            Self::WorkloadsStop | Self::DeploymentsCancel | Self::BuildRunsCancel
        );
        json!({
            "name": self.name(),
            "title": title,
            "description": description,
            "inputSchema": input_schema,
            "annotations": {
                "readOnlyHint": read_only,
                "destructiveHint": destructive,
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

fn idempotent_uuid_id_schema(property: &str) -> Value {
    let mut properties = serde_json::Map::new();
    properties.insert(property.into(), json!({"type": "string", "format": "uuid"}));
    properties.insert("idempotencyKey".into(), idempotency_key_schema());
    json!({
        "type": "object",
        "properties": properties,
        "required": [property, "idempotencyKey"],
        "additionalProperties": false
    })
}

fn rollback_workload_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workloadId": {"type": "string", "format": "uuid"},
            "sourceRevisionId": {"type": "string", "format": "uuid"},
            "idempotencyKey": idempotency_key_schema()
        },
        "required": ["workloadId", "sourceRevisionId", "idempotencyKey"],
        "additionalProperties": false
    })
}

fn idempotency_key_schema() -> Value {
    json!({
        "type": "string",
        "minLength": 1,
        "maxLength": MAXIMUM_IDEMPOTENCY_KEY_LENGTH
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

fn workload_logs_schema() -> Value {
    let mut properties = log_page_properties();
    properties.insert(
        "workloadId".into(),
        json!({"type": "string", "format": "uuid"}),
    );
    properties.insert(
        "revisionId".into(),
        json!({"type": "string", "format": "uuid"}),
    );
    json!({
        "type": "object",
        "properties": properties,
        "required": ["workloadId", "revisionId"],
        "additionalProperties": false
    })
}

fn build_run_logs_schema() -> Value {
    let mut properties = log_page_properties();
    properties.insert(
        "buildRunId".into(),
        json!({"type": "string", "format": "uuid"}),
    );
    json!({
        "type": "object",
        "properties": properties,
        "required": ["buildRunId"],
        "additionalProperties": false
    })
}

fn log_page_properties() -> serde_json::Map<String, Value> {
    serde_json::Map::from_iter([
        (
            "cursor".into(),
            json!({"type": "string", "pattern": "^v1:[0-9]+$"}),
        ),
        (
            "limit".into(),
            json!({
                "type": "integer",
                "minimum": 1,
                "maximum": MAXIMUM_LOG_LIMIT,
                "default": DEFAULT_LOG_LIMIT
            }),
        ),
        (
            "stream".into(),
            json!({"type": "string", "enum": ["stdout", "stderr"]}),
        ),
    ])
}

fn create_project_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "name": {"type": "string", "minLength": 1, "maxLength": 100},
            "idempotencyKey": idempotency_key_schema()
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
            "idempotencyKey": idempotency_key_schema()
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
