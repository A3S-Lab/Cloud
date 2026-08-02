use super::MANAGEMENT_MCP_PROTOCOL_VERSION;
use a3s_boot::{BootError, BootRequest, BootResponse, Result};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::Deserialize;
use serde_json::{json, Value};

pub const JSON_RPC_HEADER_MISMATCH: i32 = -32020;
pub const JSON_RPC_UNSUPPORTED_PROTOCOL_VERSION: i32 = -32022;
pub const JSON_RPC_INVALID_REQUEST: i32 = -32600;
pub const JSON_RPC_METHOD_NOT_FOUND: i32 = -32601;
pub const JSON_RPC_INVALID_PARAMS: i32 = -32602;
pub const JSON_RPC_INTERNAL_ERROR: i32 = -32603;
pub const JSON_RPC_PARSE_ERROR: i32 = -32700;

#[derive(Debug)]
pub struct JsonRpcRequest {
    pub id: Option<Value>,
    pub method: String,
    pub params: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyError {
    UnsupportedMediaType,
    Parse,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestMetadata {
    protocol_version: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestMetadataError {
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportMetadataError {
    HeaderMismatch,
    UnsupportedProtocolVersion(String),
}

#[derive(Debug, Deserialize)]
struct ImplementationInfo {
    name: String,
    version: String,
}

impl JsonRpcRequest {
    pub fn parse(value: Value) -> std::result::Result<Self, ()> {
        let Value::Object(mut object) = value else {
            return Err(());
        };
        if object
            .keys()
            .any(|key| !matches!(key.as_str(), "jsonrpc" | "id" | "method" | "params"))
        {
            return Err(());
        }
        if object.remove("jsonrpc") != Some(Value::String("2.0".into())) {
            return Err(());
        }
        let method = object
            .remove("method")
            .and_then(|value| value.as_str().map(str::to_owned))
            .filter(|value| !value.is_empty())
            .ok_or(())?;
        let id = object.remove("id");
        if id
            .as_ref()
            .is_some_and(|id| !id.is_string() && !id.is_number())
        {
            return Err(());
        }
        Ok(Self {
            id,
            method,
            params: object.remove("params").unwrap_or(Value::Null),
        })
    }
}

pub fn take_request_metadata(
    request: &mut JsonRpcRequest,
) -> std::result::Result<RequestMetadata, RequestMetadataError> {
    let params = request
        .params
        .as_object_mut()
        .ok_or(RequestMetadataError::Invalid)?;
    let metadata = params
        .remove("_meta")
        .ok_or(RequestMetadataError::Invalid)?;
    let metadata = metadata.as_object().ok_or(RequestMetadataError::Invalid)?;
    if metadata.len() > 32 {
        return Err(RequestMetadataError::Invalid);
    }

    let protocol_version = metadata
        .get("io.modelcontextprotocol/protocolVersion")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && value.len() <= 32)
        .ok_or(RequestMetadataError::Invalid)?;
    if let Some(client_info) = metadata.get("io.modelcontextprotocol/clientInfo") {
        let client_info = serde_json::from_value::<ImplementationInfo>(client_info.clone())
            .map_err(|_| RequestMetadataError::Invalid)?;
        if !valid_implementation_info(&client_info) {
            return Err(RequestMetadataError::Invalid);
        }
    }
    let client_capabilities = metadata
        .get("io.modelcontextprotocol/clientCapabilities")
        .and_then(Value::as_object)
        .ok_or(RequestMetadataError::Invalid)?;
    if client_capabilities.len() > 32 {
        return Err(RequestMetadataError::Invalid);
    }

    Ok(RequestMetadata {
        protocol_version: protocol_version.to_owned(),
    })
}

pub fn validate_transport_metadata(
    http: &BootRequest,
    rpc: &JsonRpcRequest,
    metadata: &RequestMetadata,
) -> std::result::Result<(), TransportMetadataError> {
    if http.header("mcp-protocol-version") != Some(metadata.protocol_version.as_str()) {
        return Err(TransportMetadataError::HeaderMismatch);
    }
    if metadata.protocol_version != MANAGEMENT_MCP_PROTOCOL_VERSION {
        return Err(TransportMetadataError::UnsupportedProtocolVersion(
            metadata.protocol_version.clone(),
        ));
    }
    if http.header("mcp-method") != Some(rpc.method.as_str()) {
        return Err(TransportMetadataError::HeaderMismatch);
    }

    let expected_name = match rpc.method.as_str() {
        "tools/call" | "prompts/get" => request_name(&rpc.params, "name")?,
        "resources/read" => request_name(&rpc.params, "uri")?,
        _ => None,
    };
    match (expected_name, http.header("mcp-name")) {
        (Some(expected), Some(actual))
            if decode_header_value(actual).as_deref() == Some(expected) =>
        {
            Ok(())
        }
        (None, None) => Ok(()),
        _ => Err(TransportMetadataError::HeaderMismatch),
    }
}

pub fn parse_body(request: &BootRequest) -> std::result::Result<Value, BodyError> {
    let content_type = request.header("content-type").unwrap_or_default();
    if !content_type
        .split(';')
        .next()
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/json"))
    {
        return Err(BodyError::UnsupportedMediaType);
    }
    serde_json::from_slice(request.body()).map_err(|_| BodyError::Parse)
}

pub fn accepts_streamable_http(request: &BootRequest) -> bool {
    let Some(accept) = request.header("accept") else {
        return false;
    };
    let mut json = false;
    let mut event_stream = false;
    for media_type in accept.split(',').filter_map(|part| part.split(';').next()) {
        let media_type = media_type.trim();
        json |= media_type.eq_ignore_ascii_case("application/json");
        event_stream |= media_type.eq_ignore_ascii_case("text/event-stream");
    }
    json && event_stream
}

pub fn has_valid_origin(request: &BootRequest) -> bool {
    let Some(origin) = request.header("origin") else {
        return true;
    };
    let Some(host) = request.header("host") else {
        return false;
    };
    let Ok(origin) = url::Url::parse(origin) else {
        return false;
    };
    if !matches!(origin.scheme(), "http" | "https") {
        return false;
    }
    let Ok(request_url) = url::Url::parse(&format!("{}://{host}", origin.scheme())) else {
        return false;
    };
    origin
        .host_str()
        .zip(request_url.host_str())
        .is_some_and(|(origin_host, request_host)| {
            origin_host.eq_ignore_ascii_case(request_host)
                && origin.port_or_known_default() == request_url.port_or_known_default()
        })
}

pub fn result_response(id: Value, result: Value) -> Result<BootResponse> {
    let result = complete_result(result)?;
    raw_json_response(200, &json!({"jsonrpc": "2.0", "id": id, "result": result}))
}

pub fn error_response(status: u16, id: Value, code: i32, message: &str) -> Result<BootResponse> {
    error_response_with_data(status, id, code, message, None)
}

pub fn error_response_with_data(
    status: u16,
    id: Value,
    code: i32,
    message: &str,
    data: Option<Value>,
) -> Result<BootResponse> {
    let mut error = json!({"code": code, "message": message});
    if let Some(data) = data {
        error
            .as_object_mut()
            .ok_or_else(|| BootError::Internal("MCP error must be an object".into()))?
            .insert("data".into(), data);
    }
    raw_json_response(
        status,
        &json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": error
        }),
    )
}

pub fn method_not_allowed_response() -> Result<BootResponse> {
    Ok(raw_headers(BootResponse::empty(405)).with_header("allow", "POST"))
}

fn raw_json_response(status: u16, body: &Value) -> Result<BootResponse> {
    Ok(raw_headers(BootResponse::json_with_status(status, body)?))
}

fn raw_headers(response: BootResponse) -> BootResponse {
    response
        .with_header("x-a3s-api-envelope", "1")
        .with_header("mcp-protocol-version", MANAGEMENT_MCP_PROTOCOL_VERSION)
        .with_header("cache-control", "no-store")
        .with_header("x-content-type-options", "nosniff")
}

fn valid_implementation_info(info: &ImplementationInfo) -> bool {
    valid_metadata_text(&info.name) && valid_metadata_text(&info.version)
}

fn valid_metadata_text(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && !value.chars().any(|character| character.is_control())
}

fn request_name<'a>(
    params: &'a Value,
    field: &str,
) -> std::result::Result<Option<&'a str>, TransportMetadataError> {
    params
        .as_object()
        .and_then(|params| params.get(field))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(Some)
        .ok_or(TransportMetadataError::HeaderMismatch)
}

fn decode_header_value(value: &str) -> Option<String> {
    if let Some(encoded) = value
        .strip_prefix("=?base64?")
        .and_then(|value| value.strip_suffix("?="))
    {
        if encoded.is_empty() {
            return None;
        }
        return String::from_utf8(STANDARD.decode(encoded).ok()?).ok();
    }
    if value.is_empty()
        || value.trim_matches([' ', '\t']) != value
        || !value
            .bytes()
            .all(|byte| byte == b'\t' || (0x20..=0x7e).contains(&byte))
    {
        return None;
    }
    Some(value.to_owned())
}

fn complete_result(mut result: Value) -> Result<Value> {
    let object = result
        .as_object_mut()
        .ok_or_else(|| BootError::Internal("MCP result must be an object".into()))?;
    object.insert("resultType".into(), Value::String("complete".into()));
    let metadata = object.entry("_meta").or_insert_with(|| json!({}));
    let metadata = metadata
        .as_object_mut()
        .ok_or_else(|| BootError::Internal("MCP result metadata must be an object".into()))?;
    metadata.insert(
        "io.modelcontextprotocol/serverInfo".into(),
        json!({"name": "a3s-cloud", "version": env!("CARGO_PKG_VERSION")}),
    );
    Ok(result)
}
