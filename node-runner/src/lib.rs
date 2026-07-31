use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use a3s_workflow_protocol::{
    NodeExecutionPhase, NodeExecutionResult, NodeInvocation, NodeKind, NodeSuspension,
};
use anyhow::{anyhow, bail, Context, Result};
use reqwest::{Client, Method, RequestBuilder};
use serde_json::{json, Map, Value};
use url::Url;

#[derive(Clone)]
pub struct NodeExecutor {
    client: Client,
}

impl NodeExecutor {
    pub fn new() -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .context("failed to build node HTTP client")?;
        Ok(Self { client })
    }

    pub async fn execute(&self, invocation: &NodeInvocation) -> Result<NodeExecutionResult> {
        invocation.validate().map_err(anyhow::Error::msg)?;
        let mut result = match invocation.node.kind {
            NodeKind::Start => NodeExecutionResult::completed(invocation.workflow_input.clone()),
            NodeKind::Template => self.execute_template(invocation)?,
            NodeKind::Llm => self.execute_llm(invocation).await?,
            NodeKind::Agent => self.execute_agent(invocation).await?,
            NodeKind::Tool | NodeKind::Http => self.execute_http(invocation).await?,
            NodeKind::Router => self.execute_router(invocation)?,
            NodeKind::Memory => self.execute_memory(invocation).await?,
            NodeKind::Approval => self.execute_approval(invocation)?,
            NodeKind::Output => self.execute_output(invocation)?,
        };
        result.metadata.insert(
            "nodeKind".to_string(),
            Value::String(invocation.node.kind.as_str().to_string()),
        );
        result.validate().map_err(anyhow::Error::msg)?;
        Ok(result)
    }

    fn rendered_config(&self, invocation: &NodeInvocation) -> Result<Value> {
        render_value(
            &invocation.node.data.config,
            &invocation.workflow_input,
            &invocation.dependencies,
        )
    }

    fn execute_template(&self, invocation: &NodeInvocation) -> Result<NodeExecutionResult> {
        let config = self.rendered_config(invocation)?;
        if let Some(value) = config.get("value") {
            return Ok(NodeExecutionResult::completed(value.clone()));
        }
        let template = required_string(&config, "template", &invocation.node.id)?;
        Ok(NodeExecutionResult::completed(json!({ "text": template })))
    }

    async fn execute_llm(&self, invocation: &NodeInvocation) -> Result<NodeExecutionResult> {
        let config = self.rendered_config(invocation)?;
        let prompt = required_string(&config, "prompt", &invocation.node.id)?;
        let model = model_for(&config, invocation)?;
        let mut messages = Vec::new();
        if let Some(system) = nonempty_string(&config, "system") {
            messages.push(json!({ "role": "system", "content": system }));
        }
        messages.push(json!({ "role": "user", "content": prompt }));

        let mut body = json!({ "model": model, "messages": messages });
        copy_optional_fields(
            &config,
            &mut body,
            &["temperature", "top_p", "max_tokens", "response_format"],
        );
        let payload = self.gateway_chat(invocation, &body).await?;
        let content = payload
            .pointer("/choices/0/message/content")
            .cloned()
            .ok_or_else(|| anyhow!("gateway response is missing choices[0].message.content"))?;
        Ok(NodeExecutionResult::completed(json!({
            "content": content,
            "model": model,
            "usage": payload.get("usage").cloned().unwrap_or(Value::Null)
        })))
    }

    async fn execute_agent(&self, invocation: &NodeInvocation) -> Result<NodeExecutionResult> {
        let config = self.rendered_config(invocation)?;
        let model = model_for(&config, invocation)?;
        let prompt = required_string(&config, "prompt", &invocation.node.id)?;
        let max_iterations = config
            .get("maxIterations")
            .and_then(Value::as_u64)
            .unwrap_or(6);
        if !(1..=16).contains(&max_iterations) {
            bail!("agent maxIterations must be between 1 and 16");
        }

        let tools = config
            .get("tools")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let public_tools = tools
            .iter()
            .map(public_tool_definition)
            .collect::<Result<Vec<_>>>()?;
        let mut messages = Vec::new();
        if let Some(system) = nonempty_string(&config, "system") {
            messages.push(json!({ "role": "system", "content": system }));
        }
        messages.push(json!({ "role": "user", "content": prompt }));
        let mut tool_results = Vec::new();

        for iteration in 1..=max_iterations {
            let mut request = json!({ "model": model, "messages": messages });
            if !public_tools.is_empty() {
                request["tools"] = Value::Array(public_tools.clone());
                request["tool_choice"] = Value::String("auto".to_string());
            }
            copy_optional_fields(
                &config,
                &mut request,
                &["temperature", "top_p", "max_tokens"],
            );
            let payload = self.gateway_chat(invocation, &request).await?;
            let message = payload
                .pointer("/choices/0/message")
                .cloned()
                .ok_or_else(|| anyhow!("gateway response is missing choices[0].message"))?;
            let calls = message
                .get("tool_calls")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            messages.push(message.clone());

            if calls.is_empty() {
                let content = message.get("content").cloned().unwrap_or(Value::Null);
                return Ok(NodeExecutionResult::completed(json!({
                    "content": content,
                    "model": model,
                    "iterations": iteration,
                    "toolResults": tool_results,
                    "usage": payload.get("usage").cloned().unwrap_or(Value::Null)
                })));
            }

            for call in calls {
                let call_id = required_json_string(&call, "id", "agent tool call")?;
                let name = call
                    .pointer("/function/name")
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow!("agent tool call is missing function.name"))?;
                let arguments = call
                    .pointer("/function/arguments")
                    .and_then(Value::as_str)
                    .unwrap_or("{}");
                let arguments: Value = serde_json::from_str(arguments)
                    .with_context(|| format!("tool {name} returned invalid JSON arguments"))?;
                let definition = tools
                    .iter()
                    .find(|tool| tool_name(tool) == Some(name))
                    .ok_or_else(|| anyhow!("agent requested unknown tool {name}"))?;
                let output = self
                    .call_tool_definition(invocation, definition, &arguments)
                    .await?;
                tool_results.push(json!({ "name": name, "output": output }));
                messages.push(json!({
                    "role": "tool",
                    "tool_call_id": call_id,
                    "content": serde_json::to_string(&output)?
                }));
            }
        }

        bail!("agent exhausted maxIterations without producing a final response")
    }

    async fn execute_http(&self, invocation: &NodeInvocation) -> Result<NodeExecutionResult> {
        let config = self.rendered_config(invocation)?;
        let result = self.call_http_config(invocation, &config).await?;
        Ok(NodeExecutionResult::completed(result))
    }

    fn execute_router(&self, invocation: &NodeInvocation) -> Result<NodeExecutionResult> {
        let config = self.rendered_config(invocation)?;
        let cases = config
            .get("routes")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow!("router node {} requires config.routes", invocation.node.id))?;
        for case in cases {
            let when = case
                .get("when")
                .and_then(Value::as_object)
                .ok_or_else(|| anyhow!("router route requires a when object"))?;
            let actual = when.get("value").unwrap_or(&Value::Null);
            let expected = when.get("equals").unwrap_or(&Value::Bool(true));
            if actual == expected {
                let route = required_json_string(case, "route", "router route")?;
                let mut result = NodeExecutionResult::completed(output_value(
                    invocation.dependencies.clone(),
                    &invocation.workflow_input,
                ));
                result.route = Some(route.to_string());
                return Ok(result);
            }
        }
        let route = required_string(&config, "default", &invocation.node.id)?;
        let mut result = NodeExecutionResult::completed(output_value(
            invocation.dependencies.clone(),
            &invocation.workflow_input,
        ));
        result.route = Some(route.to_string());
        Ok(result)
    }

    async fn execute_memory(&self, invocation: &NodeInvocation) -> Result<NodeExecutionResult> {
        let config = self.rendered_config(invocation)?;
        let base_url = invocation
            .services
            .memory_base_url
            .as_deref()
            .ok_or_else(|| anyhow!("memory node requires services.memoryBaseUrl"))?;
        let operation = config
            .get("operation")
            .and_then(Value::as_str)
            .unwrap_or("search");
        let endpoint = match operation {
            "store" | "search" => {
                format!("{}/memories:{}", base_url.trim_end_matches('/'), operation)
            }
            "retrieve" | "delete" => {
                let id = required_string(&config, "id", &invocation.node.id)?;
                format!("{}/memories/{id}", base_url.trim_end_matches('/'))
            }
            _ => bail!("unsupported memory operation {operation}"),
        };
        let method = if operation == "retrieve" {
            Method::GET
        } else if operation == "delete" {
            Method::DELETE
        } else {
            Method::POST
        };
        let mut request = self.client.request(method, endpoint);
        if matches!(operation, "store" | "search") {
            request = request.json(&config);
        }
        if let Ok(token) = std::env::var("A3S_MEMORY_API_KEY") {
            if !token.is_empty() {
                request = request.bearer_auth(token);
            }
        }
        let response = request.send().await.context("A3S Memory request failed")?;
        let status = response.status();
        let body = limited_response(response, invocation.services.max_http_response_bytes).await?;
        if !status.is_success() {
            bail!("A3S Memory returned {status}: {}", body_text(&body));
        }
        let output = serde_json::from_slice(&body).context("A3S Memory returned invalid JSON")?;
        Ok(NodeExecutionResult::completed(output))
    }

    fn execute_approval(&self, invocation: &NodeInvocation) -> Result<NodeExecutionResult> {
        if invocation.phase == NodeExecutionPhase::Resume {
            return Ok(NodeExecutionResult::completed(
                invocation.resume_payload.clone().unwrap_or(Value::Null),
            ));
        }
        let config = self.rendered_config(invocation)?;
        let message = config
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or(&invocation.node.data.label)
            .trim()
            .to_string();
        if message.is_empty() {
            bail!("approval message must not be empty");
        }
        let mut result = NodeExecutionResult::completed(Value::Null);
        result.suspension = Some(NodeSuspension::HumanApproval {
            message,
            details: config.get("details").cloned().unwrap_or(Value::Null),
        });
        Ok(result)
    }

    fn execute_output(&self, invocation: &NodeInvocation) -> Result<NodeExecutionResult> {
        let config = self.rendered_config(invocation)?;
        let output = config.get("value").cloned().unwrap_or_else(|| {
            output_value(invocation.dependencies.clone(), &invocation.workflow_input)
        });
        Ok(NodeExecutionResult::completed(output))
    }

    async fn gateway_chat(&self, invocation: &NodeInvocation, body: &Value) -> Result<Value> {
        let endpoint = format!(
            "{}/chat/completions",
            invocation.services.gateway_base_url.trim_end_matches('/')
        );
        let mut request = self.client.post(endpoint).json(body);
        if let Ok(api_key) = std::env::var("A3S_GATEWAY_API_KEY") {
            if !api_key.is_empty() {
                request = request.bearer_auth(api_key);
            }
        }
        let response = request.send().await.context("A3S Gateway request failed")?;
        let status = response.status();
        let body = limited_response(response, invocation.services.max_http_response_bytes).await?;
        if !status.is_success() {
            bail!("A3S Gateway returned {status}: {}", body_text(&body));
        }
        serde_json::from_slice(&body).context("A3S Gateway returned invalid JSON")
    }

    async fn call_tool_definition(
        &self,
        invocation: &NodeInvocation,
        definition: &Value,
        arguments: &Value,
    ) -> Result<Value> {
        let endpoint = required_json_string(definition, "endpoint", "agent tool")?;
        let config = json!({
            "url": endpoint,
            "method": definition.get("method").and_then(Value::as_str).unwrap_or("POST"),
            "headers": definition.get("headers").cloned().unwrap_or_else(|| json!({})),
            "headerEnv": definition.get("headerEnv").cloned().unwrap_or_else(|| json!({})),
            "body": arguments
        });
        self.call_http_config(invocation, &config).await
    }

    async fn call_http_config(&self, invocation: &NodeInvocation, config: &Value) -> Result<Value> {
        let raw_url = config
            .get("url")
            .or_else(|| config.get("endpoint"))
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("node {} requires config.url", invocation.node.id))?;
        let url = Url::parse(raw_url).context("invalid HTTP node URL")?;
        if !matches!(url.scheme(), "http" | "https") {
            bail!("HTTP nodes only support http:// and https:// URLs");
        }
        let host = url
            .host_str()
            .ok_or_else(|| anyhow!("HTTP node URL has no host"))?;
        let allowed = invocation
            .services
            .http_allowed_hosts
            .iter()
            .map(|value| value.to_ascii_lowercase())
            .collect::<BTreeSet<_>>();
        if !host_is_allowed(host, &allowed) {
            bail!("HTTP node host {host} is not allow-listed");
        }
        let method = config
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or("GET")
            .parse::<Method>()
            .context("invalid HTTP method")?;
        let mut request = self.client.request(method, url);
        request = apply_headers(request, config)?;
        if let Some(body) = config.get("body") {
            request = request.json(body);
        }
        let response = request.send().await.context("HTTP node request failed")?;
        let status = response.status();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        let body = limited_response(response, invocation.services.max_http_response_bytes).await?;
        let body = if content_type.contains("application/json") {
            serde_json::from_slice(&body).context("HTTP node returned invalid JSON")?
        } else {
            Value::String(body_text(&body))
        };
        if !status.is_success() {
            bail!("HTTP node returned {status}: {body}");
        }
        Ok(json!({ "status": status.as_u16(), "body": body }))
    }
}

impl Default for NodeExecutor {
    fn default() -> Self {
        Self::new().expect("the static reqwest client configuration is valid")
    }
}

fn model_for<'a>(config: &'a Value, invocation: &'a NodeInvocation) -> Result<&'a str> {
    config
        .get("model")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            (!invocation.services.default_model.trim().is_empty())
                .then_some(invocation.services.default_model.as_str())
        })
        .ok_or_else(|| anyhow!("node {} requires a model", invocation.node.id))
}

fn nonempty_string<'a>(config: &'a Value, key: &str) -> Option<&'a str> {
    config
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
}

fn required_string<'a>(config: &'a Value, key: &str, node_id: &str) -> Result<&'a str> {
    nonempty_string(config, key)
        .ok_or_else(|| anyhow!("node {node_id} requires non-empty config.{key}"))
}

fn required_json_string<'a>(value: &'a Value, key: &str, context: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow!("{context} requires a non-empty {key}"))
}

fn copy_optional_fields(source: &Value, target: &mut Value, fields: &[&str]) {
    for field in fields {
        if let Some(value) = source.get(*field) {
            target[*field] = value.clone();
        }
    }
}

fn public_tool_definition(tool: &Value) -> Result<Value> {
    let name = tool_name(tool).ok_or_else(|| anyhow!("agent tool requires function.name"))?;
    let description = tool
        .pointer("/function/description")
        .and_then(Value::as_str)
        .unwrap_or(name);
    let parameters = tool
        .pointer("/function/parameters")
        .cloned()
        .unwrap_or_else(|| json!({ "type": "object", "properties": {} }));
    Ok(json!({
        "type": "function",
        "function": {
            "name": name,
            "description": description,
            "parameters": parameters
        }
    }))
}

fn tool_name(tool: &Value) -> Option<&str> {
    tool.pointer("/function/name").and_then(Value::as_str)
}

fn apply_headers(mut request: RequestBuilder, config: &Value) -> Result<RequestBuilder> {
    if let Some(headers) = config.get("headers").and_then(Value::as_object) {
        for (name, value) in headers {
            let value = value
                .as_str()
                .ok_or_else(|| anyhow!("HTTP header {name} must be a string"))?;
            request = request.header(name, value);
        }
    }
    if let Some(headers) = config.get("headerEnv").and_then(Value::as_object) {
        for (name, variable) in headers {
            let variable = variable
                .as_str()
                .ok_or_else(|| anyhow!("headerEnv {name} must name an environment variable"))?;
            let value = std::env::var(variable)
                .with_context(|| format!("required secret environment {variable} is missing"))?;
            request = request.header(name, value);
        }
    }
    Ok(request)
}

fn host_is_allowed(host: &str, allowed: &BTreeSet<String>) -> bool {
    let host = host.to_ascii_lowercase();
    allowed.iter().any(|candidate| {
        candidate == &host
            || candidate
                .strip_prefix("*.")
                .is_some_and(|suffix| host.ends_with(&format!(".{suffix}")))
    })
}

async fn limited_response(response: reqwest::Response, limit: usize) -> Result<Vec<u8>> {
    if response
        .content_length()
        .is_some_and(|content_length| content_length > limit as u64)
    {
        bail!("response exceeds configured limit of {limit} bytes");
    }
    let bytes = response.bytes().await.context("failed to read response")?;
    if bytes.len() > limit {
        bail!("response exceeds configured limit of {limit} bytes");
    }
    Ok(bytes.to_vec())
}

fn body_text(body: &[u8]) -> String {
    String::from_utf8_lossy(body).into_owned()
}

fn output_value(mut dependencies: BTreeMap<String, Value>, workflow_input: &Value) -> Value {
    match dependencies.len() {
        0 => workflow_input.clone(),
        1 => dependencies
            .pop_first()
            .map(|(_, value)| value)
            .unwrap_or(Value::Null),
        _ => Value::Object(dependencies.into_iter().collect()),
    }
}

fn render_value(
    value: &Value,
    workflow_input: &Value,
    dependencies: &BTreeMap<String, Value>,
) -> Result<Value> {
    match value {
        Value::String(value) => render_string(value, workflow_input, dependencies),
        Value::Array(values) => values
            .iter()
            .map(|value| render_value(value, workflow_input, dependencies))
            .collect::<Result<Vec<_>>>()
            .map(Value::Array),
        Value::Object(values) => values
            .iter()
            .map(|(key, value)| {
                render_value(value, workflow_input, dependencies).map(|value| (key.clone(), value))
            })
            .collect::<Result<Map<_, _>>>()
            .map(Value::Object),
        value => Ok(value.clone()),
    }
}

fn render_string(
    source: &str,
    workflow_input: &Value,
    dependencies: &BTreeMap<String, Value>,
) -> Result<Value> {
    let trimmed = source.trim();
    if let Some(token) = whole_token(trimmed) {
        return lookup_token(token, workflow_input, dependencies).cloned();
    }
    let mut output = String::new();
    let mut remainder = source;
    while let Some(start) = remainder.find("{{") {
        output.push_str(&remainder[..start]);
        let token_source = &remainder[start + 2..];
        let end = token_source
            .find("}}")
            .ok_or_else(|| anyhow!("template contains an unclosed token"))?;
        let token = token_source[..end].trim();
        output.push_str(&scalar_text(lookup_token(
            token,
            workflow_input,
            dependencies,
        )?));
        remainder = &token_source[end + 2..];
    }
    output.push_str(remainder);
    Ok(Value::String(output))
}

fn whole_token(source: &str) -> Option<&str> {
    source
        .strip_prefix("{{")?
        .strip_suffix("}}")
        .map(str::trim)
        .filter(|token| !token.contains("}}"))
}

fn lookup_token<'a>(
    token: &str,
    workflow_input: &'a Value,
    dependencies: &'a BTreeMap<String, Value>,
) -> Result<&'a Value> {
    if token == "input" {
        return Ok(workflow_input);
    }
    if let Some(path) = token.strip_prefix("input.") {
        return lookup_path(workflow_input, path)
            .ok_or_else(|| anyhow!("template token {token} was not found"));
    }
    if let Some(path) = token.strip_prefix("steps.") {
        let (node_id, nested) = path.split_once('.').unwrap_or((path, ""));
        let value = dependencies
            .get(node_id)
            .ok_or_else(|| anyhow!("template dependency {node_id} is unavailable"))?;
        if nested.is_empty() {
            return Ok(value);
        }
        return lookup_path(value, nested)
            .ok_or_else(|| anyhow!("template token {token} was not found"));
    }
    bail!("unsupported template token {token}; use input.* or steps.<node>.*")
}

fn lookup_path<'a>(mut value: &'a Value, path: &str) -> Option<&'a Value> {
    for segment in path.split('.') {
        value = match value {
            Value::Object(object) => object.get(segment)?,
            Value::Array(values) => values.get(segment.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(value)
}

fn scalar_text(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Null => String::new(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::Array(_) | Value::Object(_) => value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use a3s_workflow_protocol::{
        NodeData, NodeRuntimePolicy, NodeServiceContext, Position, WorkflowNode,
        NODE_INVOCATION_SCHEMA,
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::task::JoinHandle;

    use super::*;

    struct MockResponse {
        status: u16,
        content_type: &'static str,
        body: String,
    }

    impl MockResponse {
        fn json(status: u16, body: Value) -> Self {
            Self {
                status,
                content_type: "application/json",
                body: body.to_string(),
            }
        }

        fn text(status: u16, body: &str) -> Self {
            Self {
                status,
                content_type: "text/plain; charset=utf-8",
                body: body.to_string(),
            }
        }
    }

    async fn spawn_server(responses: Vec<MockResponse>) -> (String, JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock server");
        let address = listener.local_addr().expect("mock server address");
        let task = tokio::spawn(async move {
            let mut requests = Vec::new();
            for response in responses {
                let (mut socket, _) = listener.accept().await.expect("accept mock request");
                let mut request = Vec::new();
                let mut buffer = [0_u8; 2048];
                loop {
                    let read = socket.read(&mut buffer).await.expect("read mock request");
                    if read == 0 {
                        break;
                    }
                    request.extend_from_slice(&buffer[..read]);
                    if request_length(&request).is_some_and(|length| request.len() >= length) {
                        break;
                    }
                }
                requests.push(String::from_utf8_lossy(&request).into_owned());
                let reason = match response.status {
                    200 => "OK",
                    201 => "Created",
                    400 => "Bad Request",
                    500 => "Internal Server Error",
                    _ => "Response",
                };
                let head = format!(
                    "HTTP/1.1 {} {}\r\ncontent-type: {}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    response.status,
                    reason,
                    response.content_type,
                    response.body.len()
                );
                socket
                    .write_all(head.as_bytes())
                    .await
                    .expect("write mock response head");
                socket
                    .write_all(response.body.as_bytes())
                    .await
                    .expect("write mock response body");
                socket.shutdown().await.expect("close mock response");
            }
            requests
        });
        (format!("http://{address}"), task)
    }

    fn request_length(request: &[u8]) -> Option<usize> {
        let source = String::from_utf8_lossy(request);
        let header_end = source.find("\r\n\r\n")? + 4;
        let content_length = source[..header_end]
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
            .unwrap_or_default();
        Some(header_end + content_length)
    }

    fn invocation(kind: NodeKind, config: Value) -> NodeInvocation {
        NodeInvocation {
            schema: NODE_INVOCATION_SCHEMA.to_string(),
            run_id: "run-1".to_string(),
            step_id: format!("node:{}:execute", kind.as_str()),
            attempt: 1,
            workflow_id: "workflow-1".to_string(),
            workflow_version: 1,
            phase: NodeExecutionPhase::Execute,
            node: WorkflowNode {
                id: kind.as_str().to_string(),
                kind,
                position: Position { x: 0.0, y: 0.0 },
                data: NodeData {
                    label: kind.as_str().to_string(),
                    config,
                    runtime: NodeRuntimePolicy::default(),
                },
            },
            workflow_input: json!({ "name": "Ada" }),
            dependencies: BTreeMap::from([("draft".to_string(), json!({ "id": 7 }))]),
            resume_payload: None,
            services: NodeServiceContext {
                gateway_base_url: "http://gateway.test/v1".to_string(),
                default_model: "test-model".to_string(),
                memory_base_url: None,
                http_allowed_hosts: Vec::new(),
                max_http_response_bytes: 1024,
            },
        }
    }

    #[tokio::test]
    async fn template_preserves_typed_tokens() {
        let result = NodeExecutor::default()
            .execute(&invocation(
                NodeKind::Template,
                json!({ "value": { "name": "{{input.name}}", "id": "{{steps.draft.id}}" } }),
            ))
            .await
            .expect("template node");
        assert_eq!(result.output, json!({ "name": "Ada", "id": 7 }));
    }

    #[tokio::test]
    async fn router_returns_an_explicit_handle() {
        let result = NodeExecutor::default()
            .execute(&invocation(
                NodeKind::Router,
                json!({
                    "routes": [{ "when": { "value": "{{input.name}}", "equals": "Ada" }, "route": "known" }],
                    "default": "unknown"
                }),
            ))
            .await
            .expect("router node");
        assert_eq!(result.route.as_deref(), Some("known"));
    }

    #[tokio::test]
    async fn approval_suspends_then_resumes() {
        let executor = NodeExecutor::default();
        let initial = executor
            .execute(&invocation(
                NodeKind::Approval,
                json!({ "message": "Ship it?" }),
            ))
            .await
            .expect("approval request");
        assert!(initial.suspension.is_some());

        let mut resumed = invocation(NodeKind::Approval, json!({ "message": "Ship it?" }));
        resumed.phase = NodeExecutionPhase::Resume;
        resumed.resume_payload = Some(json!({ "approved": true }));
        let result = executor.execute(&resumed).await.expect("approval resume");
        assert_eq!(result.output, json!({ "approved": true }));
        assert!(result.suspension.is_none());
    }

    #[tokio::test]
    async fn start_and_output_nodes_preserve_typed_values() {
        let executor = NodeExecutor::default();
        let start = executor
            .execute(&invocation(NodeKind::Start, json!({})))
            .await
            .expect("start node");
        assert_eq!(start.output, json!({ "name": "Ada" }));
        assert_eq!(start.metadata.get("nodeKind"), Some(&json!("start")));

        let mut output = invocation(NodeKind::Output, json!({}));
        output.dependencies.clear();
        assert_eq!(
            executor
                .execute(&output)
                .await
                .expect("input output")
                .output,
            output.workflow_input
        );

        output.dependencies = BTreeMap::from([("only".to_string(), json!([1, 2, 3]))]);
        assert_eq!(
            executor
                .execute(&output)
                .await
                .expect("single dependency output")
                .output,
            json!([1, 2, 3])
        );

        output
            .dependencies
            .insert("second".to_string(), json!(true));
        assert_eq!(
            executor
                .execute(&output)
                .await
                .expect("multiple dependency output")
                .output,
            json!({ "only": [1, 2, 3], "second": true })
        );

        output.node.data.config = json!({ "value": { "explicit": 42 } });
        assert_eq!(
            executor
                .execute(&output)
                .await
                .expect("explicit output")
                .output,
            json!({ "explicit": 42 })
        );
    }

    #[tokio::test]
    async fn template_interpolates_scalars_and_rejects_invalid_tokens() {
        let executor = NodeExecutor::default();
        let rendered = executor
            .execute(&invocation(
                NodeKind::Template,
                json!({ "template": "Hello {{ input.name }} #{{steps.draft.id}}" }),
            ))
            .await
            .expect("rendered template");
        assert_eq!(rendered.output, json!({ "text": "Hello Ada #7" }));

        for (template, message) in [
            ("{{input.missing}}", "was not found"),
            ("{{steps.unknown.id}}", "dependency unknown is unavailable"),
            ("{{unknown.value}}", "unsupported template token"),
            ("prefix {{input.name", "unclosed token"),
        ] {
            let error = executor
                .execute(&invocation(
                    NodeKind::Template,
                    json!({ "template": template }),
                ))
                .await
                .expect_err("invalid template must fail");
            assert!(
                error.to_string().contains(message),
                "expected {error} to contain {message:?}"
            );
        }
    }

    #[tokio::test]
    async fn router_uses_default_route_and_requires_route_configuration() {
        let executor = NodeExecutor::default();
        let defaulted = executor
            .execute(&invocation(
                NodeKind::Router,
                json!({
                    "routes": [{ "when": { "value": false }, "route": "yes" }],
                    "default": "no"
                }),
            ))
            .await
            .expect("default route");
        assert_eq!(defaulted.route.as_deref(), Some("no"));
        assert_eq!(defaulted.output, json!({ "id": 7 }));

        let error = executor
            .execute(&invocation(NodeKind::Router, json!({ "default": "no" })))
            .await
            .expect_err("missing routes must fail");
        assert!(error.to_string().contains("requires config.routes"));
    }

    #[tokio::test]
    async fn approval_uses_label_details_and_rejects_blank_message() {
        let executor = NodeExecutor::default();
        let mut request = invocation(NodeKind::Approval, json!({ "details": { "risk": "high" } }));
        request.node.data.label = "Review deployment".to_string();
        let result = executor.execute(&request).await.expect("approval");
        assert_eq!(
            result.suspension,
            Some(NodeSuspension::HumanApproval {
                message: "Review deployment".to_string(),
                details: json!({ "risk": "high" })
            })
        );

        request.node.data.label = "  ".to_string();
        let error = executor
            .execute(&request)
            .await
            .expect_err("blank approval message");
        assert!(error.to_string().contains("must not be empty"));
    }

    #[test]
    fn model_tool_and_host_helpers_enforce_safe_defaults() {
        let request = invocation(NodeKind::Llm, json!({}));
        assert_eq!(
            model_for(&json!({ "model": "explicit" }), &request).expect("explicit model"),
            "explicit"
        );
        assert_eq!(
            model_for(&json!({}), &request).expect("default model"),
            "test-model"
        );
        let mut no_model = request;
        no_model.services.default_model.clear();
        assert!(model_for(&json!({}), &no_model).is_err());

        assert_eq!(
            public_tool_definition(&json!({
                "function": {
                    "name": "lookup",
                    "description": "Looks up a value",
                    "parameters": { "type": "object" }
                },
                "endpoint": "https://tools.test/lookup"
            }))
            .expect("public tool"),
            json!({
                "type": "function",
                "function": {
                    "name": "lookup",
                    "description": "Looks up a value",
                    "parameters": { "type": "object" }
                }
            })
        );
        assert!(public_tool_definition(&json!({})).is_err());

        let allowed = BTreeSet::from([
            "api.example.test".to_string(),
            "*.tools.example.test".to_string(),
        ]);
        assert!(host_is_allowed("API.EXAMPLE.TEST", &allowed));
        assert!(host_is_allowed("worker.tools.example.test", &allowed));
        assert!(!host_is_allowed("tools.example.test", &allowed));
        assert!(!host_is_allowed("evil-example.test", &allowed));
    }

    #[tokio::test]
    async fn http_and_tool_nodes_handle_json_text_and_request_metadata() {
        let (base_url, server) = spawn_server(vec![
            MockResponse::json(200, json!({ "saved": true })),
            MockResponse::text(200, "plain response"),
        ])
        .await;
        let executor = NodeExecutor::default();
        let mut http = invocation(
            NodeKind::Http,
            json!({
                "url": format!("{base_url}/items"),
                "method": "POST",
                "headers": { "x-test": "yes" },
                "body": { "id": 7 }
            }),
        );
        http.services.http_allowed_hosts = vec!["127.0.0.1".to_string()];
        assert_eq!(
            executor.execute(&http).await.expect("HTTP node").output,
            json!({ "status": 200, "body": { "saved": true } })
        );

        let mut tool = invocation(
            NodeKind::Tool,
            json!({ "endpoint": format!("{base_url}/plain") }),
        );
        tool.services.http_allowed_hosts = vec!["127.0.0.1".to_string()];
        assert_eq!(
            executor.execute(&tool).await.expect("tool node").output,
            json!({ "status": 200, "body": "plain response" })
        );

        let requests = server.await.expect("mock server");
        assert!(requests[0].starts_with("POST /items HTTP/1.1"));
        assert!(requests[0].to_ascii_lowercase().contains("x-test: yes"));
        assert!(requests[0].contains("{\"id\":7}"));
        assert!(requests[1].starts_with("GET /plain HTTP/1.1"));
    }

    #[tokio::test]
    async fn http_nodes_reject_unsafe_targets_methods_statuses_and_large_responses() {
        let executor = NodeExecutor::default();
        let blocked = invocation(
            NodeKind::Http,
            json!({ "url": "https://blocked.example.test" }),
        );
        assert!(executor
            .execute(&blocked)
            .await
            .expect_err("blocked host")
            .to_string()
            .contains("not allow-listed"));

        let mut invalid_scheme = invocation(NodeKind::Http, json!({ "url": "file:///etc/passwd" }));
        invalid_scheme.services.http_allowed_hosts = vec!["localhost".to_string()];
        assert!(executor
            .execute(&invalid_scheme)
            .await
            .expect_err("invalid scheme")
            .to_string()
            .contains("only support http:// and https://"));

        let (base_url, server) = spawn_server(vec![
            MockResponse::json(500, json!({ "error": "failed" })),
            MockResponse::text(200, "too large"),
        ])
        .await;
        let mut failed = invocation(
            NodeKind::Http,
            json!({ "url": format!("{base_url}/failed"), "method": "GET" }),
        );
        failed.services.http_allowed_hosts = vec!["127.0.0.1".to_string()];
        assert!(executor
            .execute(&failed)
            .await
            .expect_err("failed status")
            .to_string()
            .contains("500 Internal Server Error"));

        failed.node.data.config = json!({ "url": format!("{base_url}/large"), "method": "GET" });
        failed.services.max_http_response_bytes = 4;
        assert!(executor
            .execute(&failed)
            .await
            .expect_err("large response")
            .to_string()
            .contains("exceeds configured limit"));
        server.await.expect("mock server");

        let mut invalid_method = invocation(
            NodeKind::Http,
            json!({ "url": "http://127.0.0.1", "method": "NOT A METHOD" }),
        );
        invalid_method.services.http_allowed_hosts = vec!["127.0.0.1".to_string()];
        assert!(executor
            .execute(&invalid_method)
            .await
            .expect_err("invalid method")
            .to_string()
            .contains("invalid HTTP method"));
    }

    #[tokio::test]
    async fn llm_node_calls_gateway_and_preserves_model_usage() {
        let (base_url, server) = spawn_server(vec![MockResponse::json(
            200,
            json!({
                "choices": [{ "message": { "content": { "answer": 42 } } }],
                "usage": { "total_tokens": 9 }
            }),
        )])
        .await;
        let mut request = invocation(
            NodeKind::Llm,
            json!({
                "prompt": "Answer {{input.name}}",
                "system": "Be concise",
                "temperature": 0.2
            }),
        );
        request.services.gateway_base_url = format!("{base_url}/v1");

        let output = NodeExecutor::default()
            .execute(&request)
            .await
            .expect("LLM node")
            .output;
        assert_eq!(output["content"], json!({ "answer": 42 }));
        assert_eq!(output["model"], "test-model");
        assert_eq!(output["usage"], json!({ "total_tokens": 9 }));

        let requests = server.await.expect("gateway server");
        assert!(requests[0].starts_with("POST /v1/chat/completions HTTP/1.1"));
        assert!(requests[0].contains("\"temperature\":0.2"));
        assert!(requests[0].contains("Answer Ada"));
    }

    #[tokio::test]
    async fn llm_and_agent_reject_invalid_gateway_or_iteration_contracts() {
        let (base_url, server) =
            spawn_server(vec![MockResponse::json(200, json!({ "choices": [] }))]).await;
        let mut llm = invocation(NodeKind::Llm, json!({ "prompt": "hello" }));
        llm.services.gateway_base_url = base_url;
        assert!(NodeExecutor::default()
            .execute(&llm)
            .await
            .expect_err("missing gateway content")
            .to_string()
            .contains("missing choices[0].message.content"));
        server.await.expect("gateway server");

        let agent = invocation(
            NodeKind::Agent,
            json!({ "prompt": "hello", "maxIterations": 17 }),
        );
        assert!(NodeExecutor::default()
            .execute(&agent)
            .await
            .expect_err("invalid max iterations")
            .to_string()
            .contains("between 1 and 16"));
    }

    #[tokio::test]
    async fn agent_completes_a_tool_loop_with_bounded_iterations() {
        let (tool_url, tool_server) =
            spawn_server(vec![MockResponse::json(200, json!({ "temperature": 21 }))]).await;
        let (gateway_url, gateway_server) = spawn_server(vec![
            MockResponse::json(
                200,
                json!({
                    "choices": [{
                        "message": {
                            "role": "assistant",
                            "content": null,
                            "tool_calls": [{
                                "id": "call-1",
                                "function": {
                                    "name": "weather",
                                    "arguments": "{\"city\":\"Shanghai\"}"
                                }
                            }]
                        }
                    }]
                }),
            ),
            MockResponse::json(
                200,
                json!({
                    "choices": [{
                        "message": { "role": "assistant", "content": "21 C" }
                    }],
                    "usage": { "total_tokens": 12 }
                }),
            ),
        ])
        .await;
        let mut agent = invocation(
            NodeKind::Agent,
            json!({
                "prompt": "Weather?",
                "maxIterations": 2,
                "tools": [{
                    "function": {
                        "name": "weather",
                        "description": "Get weather",
                        "parameters": {
                            "type": "object",
                            "properties": { "city": { "type": "string" } }
                        }
                    },
                    "endpoint": format!("{tool_url}/weather"),
                    "method": "POST"
                }]
            }),
        );
        agent.services.gateway_base_url = gateway_url;
        agent.services.http_allowed_hosts = vec!["127.0.0.1".to_string()];

        let output = NodeExecutor::default()
            .execute(&agent)
            .await
            .expect("agent tool loop")
            .output;
        assert_eq!(output["content"], "21 C");
        assert_eq!(output["iterations"], 2);
        assert_eq!(output["toolResults"][0]["name"], "weather");
        assert_eq!(output["toolResults"][0]["output"]["status"], 200);

        let tool_requests = tool_server.await.expect("tool server");
        assert!(tool_requests[0].starts_with("POST /weather HTTP/1.1"));
        assert!(tool_requests[0].contains("Shanghai"));
        let gateway_requests = gateway_server.await.expect("gateway server");
        assert_eq!(gateway_requests.len(), 2);
        assert!(gateway_requests[1].contains("tool_call_id"));
    }

    #[tokio::test]
    async fn memory_node_supports_store_retrieve_and_rejects_invalid_operations() {
        let (base_url, server) = spawn_server(vec![
            MockResponse::json(201, json!({ "id": "memory-1" })),
            MockResponse::json(200, json!({ "id": "memory-1", "text": "hello" })),
        ])
        .await;
        let executor = NodeExecutor::default();
        let mut store = invocation(
            NodeKind::Memory,
            json!({ "operation": "store", "text": "hello" }),
        );
        store.services.memory_base_url = Some(format!("{base_url}/api/v1"));
        assert_eq!(
            executor.execute(&store).await.expect("store memory").output,
            json!({ "id": "memory-1" })
        );

        let mut retrieve = invocation(
            NodeKind::Memory,
            json!({ "operation": "retrieve", "id": "memory-1" }),
        );
        retrieve.services.memory_base_url = store.services.memory_base_url.clone();
        assert_eq!(
            executor
                .execute(&retrieve)
                .await
                .expect("retrieve memory")
                .output,
            json!({ "id": "memory-1", "text": "hello" })
        );
        let requests = server.await.expect("memory server");
        assert!(requests[0].starts_with("POST /api/v1/memories:store HTTP/1.1"));
        assert!(requests[1].starts_with("GET /api/v1/memories/memory-1 HTTP/1.1"));

        let missing_service = invocation(NodeKind::Memory, json!({ "operation": "search" }));
        assert!(executor
            .execute(&missing_service)
            .await
            .expect_err("missing memory URL")
            .to_string()
            .contains("requires services.memoryBaseUrl"));

        let mut invalid = missing_service;
        invalid.services.memory_base_url = Some("http://memory.test".to_string());
        invalid.node.data.config = json!({ "operation": "truncate" });
        assert!(executor
            .execute(&invalid)
            .await
            .expect_err("invalid memory operation")
            .to_string()
            .contains("unsupported memory operation"));
    }
}
