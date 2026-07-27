use super::catalog::ManagementTool;
use super::projects::{
    self, CreateEnvironmentArguments, CreateProjectArguments, EmptyArguments, ProjectArguments,
};
use super::protocol::{
    self, BodyError, JsonRpcRequest, JSON_RPC_INTERNAL_ERROR, JSON_RPC_INVALID_PARAMS,
    JSON_RPC_INVALID_REQUEST, JSON_RPC_METHOD_NOT_FOUND, JSON_RPC_PARSE_ERROR,
};
use super::search::{self, SearchArguments};
use super::MANAGEMENT_MCP_PROTOCOL_VERSION;
use crate::modules::shared_kernel::domain::OrganizationId;
use a3s_boot::{AuthPrincipal, BootError, BootRequest, BootResponse, CommandBus, QueryBus, Result};
use serde::de::DeserializeOwned;
use serde::Deserialize;
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
        let parsed = match JsonRpcRequest::parse(body) {
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

        if parsed.method == "initialize" {
            return self.initialize(parsed);
        }
        if !protocol::negotiated_version(&request) {
            return protocol::error_response(
                400,
                id,
                JSON_RPC_INVALID_REQUEST,
                "Missing or unsupported MCP-Protocol-Version",
            );
        }
        if parsed.id.is_none() {
            return self.notification(parsed);
        }

        match parsed.method.as_str() {
            "ping" => protocol::result_response(id, json!({})),
            "tools/list" => self.list_tools(id, parsed.params, &principal),
            "tools/call" => {
                self.call_tool(id, parsed.params, principal, request_id)
                    .await
            }
            _ => protocol::error_response(200, id, JSON_RPC_METHOD_NOT_FOUND, "Method not found"),
        }
    }

    fn initialize(&self, request: JsonRpcRequest) -> Result<BootResponse> {
        let Some(id) = request.id else {
            return protocol::accepted_response();
        };
        let arguments = match parse_arguments::<InitializeArguments>(request.params) {
            Ok(arguments) => arguments,
            Err(()) => {
                return protocol::error_response(
                    200,
                    id,
                    JSON_RPC_INVALID_PARAMS,
                    "Invalid initialize parameters",
                )
            }
        };
        if !arguments.is_valid() {
            return protocol::error_response(
                200,
                id,
                JSON_RPC_INVALID_PARAMS,
                "Invalid initialize parameters",
            );
        }
        protocol::result_response(
            id,
            json!({
                "protocolVersion": MANAGEMENT_MCP_PROTOCOL_VERSION,
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {"name": "a3s-cloud", "version": env!("CARGO_PKG_VERSION")},
                "instructions": "Use tools/list to discover tenant-scoped Cloud management tools. Mutation visibility follows the current API-token scopes."
            }),
        )
    }

    fn notification(&self, _request: JsonRpcRequest) -> Result<BootResponse> {
        protocol::accepted_response()
    }

    fn list_tools(
        &self,
        id: Value,
        params: Value,
        principal: &AuthPrincipal,
    ) -> Result<BootResponse> {
        let arguments = parse_optional_arguments::<ListToolsArguments>(params);
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
        let call = match parse_arguments::<CallToolArguments>(params) {
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
        let arguments = call.arguments.unwrap_or_else(|| json!({}));
        let result = match tool {
            ManagementTool::EnvironmentsCreate => {
                let arguments = match parse_arguments::<CreateEnvironmentArguments>(arguments) {
                    Ok(arguments) => arguments,
                    Err(()) => return invalid_tool_arguments(id),
                };
                projects::create_environment(
                    Arc::clone(&self.command_bus),
                    organization_id,
                    arguments,
                    request_id,
                )
                .await
            }
            ManagementTool::EnvironmentsList => {
                let arguments = match parse_arguments::<ProjectArguments>(arguments) {
                    Ok(arguments) => arguments,
                    Err(()) => return invalid_tool_arguments(id),
                };
                projects::list_environments(
                    Arc::clone(&self.query_bus),
                    organization_id,
                    arguments,
                    request_id,
                )
                .await
            }
            ManagementTool::ProjectsCreate => {
                let arguments = match parse_arguments::<CreateProjectArguments>(arguments) {
                    Ok(arguments) => arguments,
                    Err(()) => return invalid_tool_arguments(id),
                };
                projects::create_project(
                    Arc::clone(&self.command_bus),
                    organization_id,
                    arguments,
                    request_id,
                )
                .await
            }
            ManagementTool::ProjectsList => {
                let arguments = match parse_arguments::<EmptyArguments>(arguments) {
                    Ok(arguments) => arguments,
                    Err(()) => return invalid_tool_arguments(id),
                };
                projects::list_projects(
                    Arc::clone(&self.query_bus),
                    organization_id,
                    arguments,
                    request_id,
                )
                .await
            }
            ManagementTool::Search => {
                let arguments = match parse_arguments::<SearchArguments>(arguments) {
                    Ok(arguments) => arguments,
                    Err(()) => return invalid_tool_arguments(id),
                };
                search::search(
                    Arc::clone(&self.query_bus),
                    organization_id,
                    arguments,
                    request_id,
                )
                .await
            }
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InitializeArguments {
    protocol_version: String,
    capabilities: serde_json::Map<String, Value>,
    client_info: ImplementationInfo,
}

impl InitializeArguments {
    fn is_valid(&self) -> bool {
        !self.protocol_version.is_empty()
            && self.protocol_version.len() <= 32
            && self.capabilities.len() <= 32
            && self.client_info.is_valid()
    }
}

#[derive(Debug, Deserialize)]
struct ImplementationInfo {
    name: String,
    version: String,
}

impl ImplementationInfo {
    fn is_valid(&self) -> bool {
        !self.name.is_empty()
            && self.name.len() <= 128
            && !self.version.is_empty()
            && self.version.len() <= 128
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ListToolsArguments {
    #[serde(default)]
    cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
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

fn parse_arguments<T>(value: Value) -> std::result::Result<T, ()>
where
    T: DeserializeOwned,
{
    serde_json::from_value(value).map_err(|_| ())
}

fn parse_optional_arguments<T>(value: Value) -> std::result::Result<T, ()>
where
    T: DeserializeOwned,
{
    parse_arguments(if value.is_null() { json!({}) } else { value })
}

fn invalid_tool_arguments(id: Value) -> Result<BootResponse> {
    protocol::error_response(200, id, JSON_RPC_INVALID_PARAMS, "Invalid tool arguments")
}
