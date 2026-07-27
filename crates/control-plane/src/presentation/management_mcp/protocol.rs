use super::MANAGEMENT_MCP_PROTOCOL_VERSION;
use a3s_boot::{BootRequest, BootResponse, Result};
use serde_json::{json, Value};

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

pub fn negotiated_version(request: &BootRequest) -> bool {
    request.header("mcp-protocol-version") == Some(MANAGEMENT_MCP_PROTOCOL_VERSION)
}

pub fn result_response(id: Value, result: Value) -> Result<BootResponse> {
    raw_json_response(200, &json!({"jsonrpc": "2.0", "id": id, "result": result}))
}

pub fn error_response(status: u16, id: Value, code: i32, message: &str) -> Result<BootResponse> {
    raw_json_response(
        status,
        &json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {"code": code, "message": message}
        }),
    )
}

pub fn accepted_response() -> Result<BootResponse> {
    Ok(raw_headers(BootResponse::empty(202)))
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
