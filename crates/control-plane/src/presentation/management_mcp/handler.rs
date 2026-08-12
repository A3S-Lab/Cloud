use super::arguments::{parse, parse_optional, EmptyArguments};
use super::catalog::{ManagementResourceBinding, ManagementTool};
use super::dispatch;
use super::protocol::{
    self, BodyError, JsonRpcRequest, RequestMetadataError, TransportMetadataError,
    JSON_RPC_HEADER_MISMATCH, JSON_RPC_INTERNAL_ERROR, JSON_RPC_INVALID_PARAMS,
    JSON_RPC_INVALID_REQUEST, JSON_RPC_METHOD_NOT_FOUND, JSON_RPC_PARSE_ERROR,
    JSON_RPC_UNSUPPORTED_PROTOCOL_VERSION,
};
use super::MANAGEMENT_MCP_PROTOCOL_VERSION;
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::identity::domain::value_objects::ResourceGrantScope;
use crate::modules::identity::presentation::resource_access_evaluator;
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, NodeId, OrganizationId, PrincipalId, ProjectId,
};
use a3s_boot::{AuthPrincipal, BootError, BootRequest, BootResponse, CommandBus, QueryBus, Result};
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Clone)]
pub struct ManagementMcpHandler {
    command_bus: Arc<CommandBus>,
    query_bus: Arc<QueryBus>,
}

impl ManagementMcpHandler {
    pub fn new(command_bus: Arc<CommandBus>, query_bus: Arc<QueryBus>) -> Self {
        Self {
            command_bus,
            query_bus,
        }
    }

    pub async fn handle(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        if !protocol::has_valid_origin(&request) {
            return protocol::error_response(
                403,
                Value::Null,
                JSON_RPC_INVALID_REQUEST,
                "Origin is not allowed",
            );
        }
        if !protocol::accepts_streamable_http(&request) {
            return protocol::error_response(
                406,
                Value::Null,
                JSON_RPC_INVALID_REQUEST,
                "Accept must include application/json and text/event-stream",
            );
        }
        let body = match protocol::parse_body(&request) {
            Ok(body) => body,
            Err(BodyError::UnsupportedMediaType) => {
                return protocol::error_response(
                    415,
                    Value::Null,
                    JSON_RPC_INVALID_REQUEST,
                    "Content-Type must be application/json",
                )
            }
            Err(BodyError::Parse) => {
                return protocol::error_response(
                    400,
                    Value::Null,
                    JSON_RPC_PARSE_ERROR,
                    "Parse error",
                )
            }
        };
        if body.is_array() {
            return protocol::error_response(
                400,
                Value::Null,
                JSON_RPC_INVALID_REQUEST,
                "Batch requests are not supported",
            );
        }
        let mut parsed = match JsonRpcRequest::parse(body) {
            Ok(parsed) => parsed,
            Err(()) => {
                return protocol::error_response(
                    400,
                    Value::Null,
                    JSON_RPC_INVALID_REQUEST,
                    "Invalid Request",
                )
            }
        };
        let id = parsed.id.clone().unwrap_or(Value::Null);
        let principal = request.require_auth_principal()?;
        let metadata = match protocol::take_request_metadata(&mut parsed) {
            Ok(metadata) => metadata,
            Err(RequestMetadataError::Invalid) => {
                return protocol::error_response(
                    400,
                    id,
                    JSON_RPC_INVALID_PARAMS,
                    "Invalid request metadata",
                )
            }
        };
        match protocol::validate_transport_metadata(&request, &parsed, &metadata) {
            Ok(()) => {}
            Err(TransportMetadataError::HeaderMismatch) => {
                return protocol::error_response(
                    400,
                    id,
                    JSON_RPC_HEADER_MISMATCH,
                    "Header mismatch",
                )
            }
            Err(TransportMetadataError::UnsupportedProtocolVersion(requested)) => {
                return protocol::error_response_with_data(
                    400,
                    id,
                    JSON_RPC_UNSUPPORTED_PROTOCOL_VERSION,
                    "Unsupported protocol version",
                    Some(json!({
                        "supported": [MANAGEMENT_MCP_PROTOCOL_VERSION],
                        "requested": requested
                    })),
                )
            }
        }
        if parsed.id.is_none() {
            return protocol::error_response(
                400,
                Value::Null,
                JSON_RPC_INVALID_REQUEST,
                "Notifications are not supported",
            );
        }

        match parsed.method.as_str() {
            "server/discover" => self.discover(id, parsed.params),
            "ping" => self.ping(id, parsed.params),
            "tools/list" => self.list_tools(id, parsed.params, &principal),
            "tools/call" => {
                self.call_tool(id, parsed.params, principal, request_id)
                    .await
            }
            _ => protocol::error_response(404, id, JSON_RPC_METHOD_NOT_FOUND, "Method not found"),
        }
    }

    fn discover(&self, id: Value, params: Value) -> Result<BootResponse> {
        if parse::<EmptyArguments>(params).is_err() {
            return protocol::error_response(
                200,
                id,
                JSON_RPC_INVALID_PARAMS,
                "Invalid server/discover parameters",
            );
        }
        protocol::result_response(
            id,
            json!({
                "supportedVersions": [MANAGEMENT_MCP_PROTOCOL_VERSION],
                "capabilities": {"tools": {}},
                "instructions": "Use tools/list to discover tenant-scoped Cloud management tools. Mutation visibility follows the current API-token scopes.",
                "ttlMs": 0,
                "cacheScope": "private"
            }),
        )
    }

    fn ping(&self, id: Value, params: Value) -> Result<BootResponse> {
        if parse::<EmptyArguments>(params).is_err() {
            return protocol::error_response(
                200,
                id,
                JSON_RPC_INVALID_PARAMS,
                "Invalid ping parameters",
            );
        }
        protocol::result_response(id, json!({}))
    }

    fn list_tools(
        &self,
        id: Value,
        params: Value,
        principal: &AuthPrincipal,
    ) -> Result<BootResponse> {
        let arguments = parse_optional::<ListToolsArguments>(params);
        if arguments.is_err() || arguments.is_ok_and(|arguments| arguments.cursor.is_some()) {
            return protocol::error_response(
                200,
                id,
                JSON_RPC_INVALID_PARAMS,
                "Invalid tools/list parameters",
            );
        }
        protocol::result_response(
            id,
            json!({"tools": ManagementTool::visible_catalog(principal)}),
        )
    }

    async fn call_tool(
        &self,
        id: Value,
        params: Value,
        principal: AuthPrincipal,
        request_id: Uuid,
    ) -> Result<BootResponse> {
        let call = match parse::<CallToolArguments>(params) {
            Ok(call) => call,
            Err(()) => {
                return protocol::error_response(
                    200,
                    id,
                    JSON_RPC_INVALID_PARAMS,
                    "Invalid tools/call parameters",
                )
            }
        };
        let Some(tool) = ManagementTool::resolve(&call.name, &principal) else {
            return protocol::error_response(
                200,
                id,
                JSON_RPC_INVALID_PARAMS,
                "Unknown or unavailable tool",
            );
        };
        let organization_id = match organization_id(&principal) {
            Ok(organization_id) => organization_id,
            Err(error) => {
                tracing::error!(%request_id, %error, "management MCP principal is invalid");
                return protocol::error_response(
                    200,
                    id,
                    JSON_RPC_INTERNAL_ERROR,
                    "Internal error",
                );
            }
        };
        let actor_principal_id = match principal_id(&principal) {
            Ok(principal_id) => principal_id,
            Err(error) => {
                tracing::error!(%request_id, %error, "management MCP principal is invalid");
                return protocol::error_response(
                    200,
                    id,
                    JSON_RPC_INTERNAL_ERROR,
                    "Internal error",
                );
            }
        };
        let actor_is_platform_admin = principal.has_role("platform_admin");
        let arguments = call.arguments.unwrap_or_else(|| json!({}));
        let evaluator = resource_access_evaluator(&principal)?;
        let authorized = match management_resource_is_authorized(tool, &arguments, &evaluator) {
            Ok(authorized) => authorized,
            Err(ManagementResourceAuthorizationError::InvalidArguments) => {
                return invalid_tool_arguments(id)
            }
        };
        if !authorized {
            return protocol::result_response(
                id,
                super::tool_result::application_error(
                    ApplicationError::NotFound("resource not found".into()),
                    request_id,
                )?,
            );
        }
        let Some(result) = dispatch::execute(
            tool,
            Arc::clone(&self.command_bus),
            Arc::clone(&self.query_bus),
            dispatch::ManagementExecutionContext::new(
                organization_id,
                actor_principal_id,
                actor_is_platform_admin,
                request_id,
                evaluator,
            ),
            arguments,
        )
        .await
        else {
            return invalid_tool_arguments(id);
        };
        match result {
            Ok(result) => protocol::result_response(id, result),
            Err(error) => {
                tracing::error!(%request_id, %error, "management MCP tool execution failed");
                protocol::error_response(200, id, JSON_RPC_INTERNAL_ERROR, "Internal error")
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ManagementResourceAuthorizationError {
    InvalidArguments,
}

fn management_resource_is_authorized(
    tool: ManagementTool,
    arguments: &Value,
    evaluator: &ResourceAccessEvaluator,
) -> std::result::Result<bool, ManagementResourceAuthorizationError> {
    let uuid_argument = |name: &str| {
        arguments
            .get(name)
            .and_then(Value::as_str)
            .and_then(|value| Uuid::parse_str(value).ok())
            .ok_or(ManagementResourceAuthorizationError::InvalidArguments)
    };
    match tool.resource_binding() {
        Some(ManagementResourceBinding::ProjectArgument) => {
            Ok(evaluator.allows(ResourceGrantScope::Project {
                project_id: ProjectId::from_uuid(uuid_argument("projectId")?),
            }))
        }
        Some(ManagementResourceBinding::EnvironmentArguments) => {
            Ok(evaluator.allows(ResourceGrantScope::Environment {
                project_id: ProjectId::from_uuid(uuid_argument("projectId")?),
                environment_id: EnvironmentId::from_uuid(uuid_argument("environmentId")?),
            }))
        }
        Some(ManagementResourceBinding::NodeArgument) => {
            Ok(evaluator.allows(ResourceGrantScope::Node {
                node_id: NodeId::from_uuid(uuid_argument("nodeId")?),
            }))
        }
        Some(ManagementResourceBinding::ProjectCollection) => {
            Ok(evaluator.has_project_visibility())
        }
        Some(ManagementResourceBinding::EnvironmentCollection) => Ok(evaluator
            .project_is_visible_in_collection(ProjectId::from_uuid(uuid_argument("projectId")?))),
        Some(ManagementResourceBinding::NodeCollection) => Ok(evaluator.has_node_visibility()),
        Some(ManagementResourceBinding::SearchCollection) => {
            Ok(evaluator.has_any_visible_resource())
        }
        None => Ok(evaluator.is_organization_wide()),
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ListToolsArguments {
    #[serde(default)]
    cursor: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CallToolArguments {
    name: String,
    #[serde(default)]
    arguments: Option<Value>,
}

fn request_id(request: &BootRequest) -> Result<Uuid> {
    request
        .header("x-request-id")
        .ok_or_else(|| BootError::Internal("request ID middleware did not run".into()))
        .and_then(|value| {
            Uuid::parse_str(value)
                .map_err(|error| BootError::Internal(format!("invalid request ID: {error}")))
        })
}

fn organization_id(principal: &AuthPrincipal) -> Result<OrganizationId> {
    principal
        .claim("organization_id")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            BootError::Internal("authenticated principal has no organization context".into())
        })
        .and_then(|value| {
            Uuid::parse_str(value)
                .map(OrganizationId::from_uuid)
                .map_err(|error| {
                    BootError::Internal(format!(
                        "authenticated organization claim is invalid: {error}"
                    ))
                })
        })
}

fn principal_id(principal: &AuthPrincipal) -> Result<PrincipalId> {
    Uuid::parse_str(principal.subject())
        .map(PrincipalId::from_uuid)
        .map_err(|error| {
            BootError::Internal(format!(
                "authenticated principal identity is invalid: {error}"
            ))
        })
}

fn invalid_tool_arguments(id: Value) -> Result<BootResponse> {
    protocol::error_response(200, id, JSON_RPC_INVALID_PARAMS, "Invalid tool arguments")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_tool_arguments_are_authorized_by_closed_resource_scopes() {
        let project_id = Uuid::now_v7();
        let environment_id = Uuid::now_v7();
        let node_id = Uuid::now_v7();
        let evaluator = ResourceAccessEvaluator::restricted([
            ResourceGrantScope::Project {
                project_id: ProjectId::from_uuid(project_id),
            },
            ResourceGrantScope::Node {
                node_id: NodeId::from_uuid(node_id),
            },
        ]);
        assert_eq!(
            management_resource_is_authorized(
                ManagementTool::FormsList,
                &json!({"projectId": project_id}),
                &evaluator,
            ),
            Ok(true)
        );
        assert_eq!(
            management_resource_is_authorized(
                ManagementTool::WorkloadsList,
                &json!({"projectId": project_id, "environmentId": environment_id}),
                &evaluator,
            ),
            Ok(true)
        );
        assert_eq!(
            management_resource_is_authorized(
                ManagementTool::NodesGet,
                &json!({"nodeId": node_id}),
                &evaluator,
            ),
            Ok(true)
        );
    }

    #[test]
    fn collection_tools_use_visibility_without_becoming_mutation_authority() {
        let project_id = Uuid::now_v7();
        let environment_id = Uuid::now_v7();
        let evaluator = ResourceAccessEvaluator::restricted([ResourceGrantScope::Environment {
            project_id: ProjectId::from_uuid(project_id),
            environment_id: EnvironmentId::from_uuid(environment_id),
        }]);
        assert_eq!(
            management_resource_is_authorized(ManagementTool::ProjectsList, &json!({}), &evaluator,),
            Ok(true)
        );
        assert_eq!(
            management_resource_is_authorized(
                ManagementTool::EnvironmentsList,
                &json!({"projectId": project_id}),
                &evaluator,
            ),
            Ok(true)
        );
        assert_eq!(
            management_resource_is_authorized(
                ManagementTool::EnvironmentsCreate,
                &json!({"projectId": project_id}),
                &evaluator,
            ),
            Ok(false)
        );
        assert_eq!(
            management_resource_is_authorized(ManagementTool::Search, &json!({}), &evaluator),
            Ok(true)
        );
    }

    #[test]
    fn indirect_and_malformed_tools_fail_closed() {
        let evaluator = ResourceAccessEvaluator::restricted([]);
        assert_eq!(
            management_resource_is_authorized(
                ManagementTool::FormsGet,
                &json!({"formId": Uuid::now_v7()}),
                &evaluator,
            ),
            Ok(false)
        );
        assert_eq!(
            management_resource_is_authorized(
                ManagementTool::FormsList,
                &json!({"projectId": "bad"}),
                &evaluator,
            ),
            Err(ManagementResourceAuthorizationError::InvalidArguments)
        );
    }
}
